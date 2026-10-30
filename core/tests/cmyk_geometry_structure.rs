//! A structured CMYK document through geometry (#1 phase 4b): a Pass
//! Through group (masked, opacity 0.6) holding a clip unit with a Multiply
//! member, a Fill 0.5 raster and a masked Levels record go through Crop,
//! Image Size, Canvas Size, a quarter turn, a flip, Straighten, Perspective
//! Crop and an `affine_set` on the group. Each time the tree — names, ids,
//! kinds, modes, clipping, Fill, opacity and which records carry masks —
//! survives, the composite equals 4a's recursive f64 oracle
//! (`common/cmyk_structure_fixture.rs`) over the transformed leaves within
//! 2/65535, and the `.rz` save writes word 84 and reopens bit-exact.

use doctored_core::doc::BlendMode;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use serde_json::json;
use tempfile::TempDir;

use crate::cmyk_geometry_fixture::turn;
use crate::cmyk_structure_fixture::{
    assert_composite, cmyk_layers, cmyk_word, composite, free, open, ramp, save, then, try_edit,
    varied, with_fill, with_levels, with_mask, with_mode, with_opacity, RgbLayer,
};

const W: u32 = 16;
const H: u32 = 12;

fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

/// Bottom; group "Pass" (Pass Through, masked, 0.6) of A and B, B a 9 × 6
/// rect clipped to A at Multiply; C at Fill 0.5; a masked Levels record.
fn structured() -> *mut PrecisionDocument {
    let full = |name, seed| RgbLayer {
        name,
        w: W,
        h: H,
        offset: (0, 0),
        pixels: varied(W, H, seed),
    };
    let mut bottom = varied(W, H, 1);
    for p in &mut bottom {
        p[3] = 65535;
    }
    let mut doc = cmyk_layers(
        W,
        H,
        vec![
            RgbLayer {
                pixels: bottom,
                ..full("Bottom", 1)
            },
            full("A", 2),
            RgbLayer {
                name: "B",
                w: 9,
                h: 6,
                offset: (3, 2),
                pixels: varied(9, 6, 3),
            },
            full("C", 4),
        ],
    );
    let (a, b) = (index(doc, "A"), index(doc, "B"));
    doc = then(
        doc,
        &format!(r#"{{"op":"group_layers","layers":[{a},{b}],"name":"Pass","pass_through":true}}"#),
        &[],
    );
    let b = index(doc, "B");
    doc = then(
        doc,
        &format!(r#"{{"op":"layer_clipped","layer":{b},"clipped":true}}"#),
        &[],
    );
    doc = with_mode(doc, b, BlendMode::Multiply.to_c());
    let pass = index(doc, "Pass");
    doc = with_mask(doc, pass, &ramp(W, H, 5));
    doc = with_opacity(doc, pass, 0.6);
    let c = index(doc, "C");
    doc = with_fill(doc, c, 0.5);
    doc = with_levels(doc, 1.0);
    let levels = index(doc, "Levels");
    with_mask(doc, levels, &ramp(W, H, 8))
}

/// What geometry must not touch: per record (postorder walk) its name, id,
/// opacity, visibility, mode, clipping, Pass Through, Fill, kind and whether
/// it carries a mask.
fn shape(layers: &[PrecisionDocumentLayer], out: &mut Vec<String>) {
    for layer in layers {
        if let Some(children) = layer.children() {
            shape(children, out);
        }
        out.push(format!(
            "{} {} {} {} {:?} {} {} {:?} {} {} {}",
            layer.name(),
            layer.id(),
            layer.opacity(),
            layer.visible(),
            layer.blend_mode(),
            layer.clipped(),
            layer.is_pass_through(),
            layer.style().map(|s| s.fill_opacity),
            layer.cmyk_source().is_some(),
            layer.children().is_some(),
            layer.mask().is_some(),
        ));
    }
}

fn shape_of(doc: *const PrecisionDocument) -> Vec<String> {
    let mut out = Vec::new();
    shape(unsafe { &*doc }.root_layers(), &mut out);
    out
}

/// Every record's stored pixels, offsets and masks equal (the round trip).
fn same_records(a: *const PrecisionDocument, b: *const PrecisionDocument, what: &str) {
    let (a, b) = (unsafe { &*a }, unsafe { &*b });
    assert_eq!(a.dimensions(), b.dimensions(), "{what}");
    for i in 0..a.layer_count() {
        let (la, lb) = (a.layer(i).unwrap(), b.layer(i).unwrap());
        assert_eq!(la.offset(), lb.offset(), "{what}: {}", la.name());
        match (la.cmyk_source(), lb.cmyk_source()) {
            (None, None) => {}
            (Some(x), Some(y)) => assert!(x.same_samples(y), "{what}: {}", la.name()),
            _ => panic!("{what}: {} changed kind", la.name()),
        }
        match (la.mask(), lb.mask()) {
            (None, None) => {}
            (Some(x), Some(y)) => {
                assert_eq!(x.properties().offset, y.properties().offset, "{what}");
                assert_eq!(x.plane().samples(), y.plane().samples(), "{what}");
            }
            _ => panic!("{what}: {} mask", la.name()),
        }
    }
}

#[test]
fn a_structured_document_survives_every_geometry_op() {
    let dir = TempDir::new().unwrap();
    let doc = structured();
    let before = shape_of(doc);
    assert_composite(doc, 2, "before");
    let pass = index(doc, "Pass");
    let requests = [
        json!({"op":"crop","x":2,"y":1,"width":12,"height":9}),
        json!({"op":"resize","width":21,"height":15,"filter":"bicubic"}),
        json!({"op":"canvas_size","width":20,"height":14,"x":2,"y":-1}),
        json!({"op":"rotate90"}),
        json!({"op":"flip_horizontal"}),
        json!({"op":"straighten","matrix":turn(11.0, 1.0, (8.0, 6.0)),"filter":"bilinear",
            "x":1,"y":1,"width":13,"height":9}),
        json!({"op":"perspective_crop","quad":[1.0,1.0,15.0,2.0,14.0,11.0,2.0,10.0],
            "width":14,"height":10,"filter":"bilinear"}),
        json!({"op":"affine_set","layers":[pass],"matrix":turn(8.0, 0.9, (8.0, 6.0)),
            "filter":"bilinear"}),
    ];
    for (n, request) in requests.iter().enumerate() {
        let what = request.to_string();
        let out = try_edit(doc, &what, &[]).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(!out.is_null(), "{what} changed nothing");
        assert!(unsafe { &*out }.is_cmyk(), "{what}");
        assert_eq!(shape_of(out), before, "{what}: the tree");
        assert_composite(out, 2, &what);
        let path = dir.path().join(format!("geometry-{n}.rz"));
        assert_eq!(cmyk_word(&save(out, &path)), 84, "{what}");
        let again = open(&path).unwrap();
        assert_eq!(shape_of(again), before, "{what}: the reopened tree");
        same_records(out, again, &what);
        let (w, h) = unsafe { &*out }.dimensions();
        for (x, y) in [
            (0, 0),
            (w as i32 - 1, h as i32 - 1),
            (w as i32 / 2, h as i32 / 2),
        ] {
            assert_eq!(
                composite(out, x, y),
                composite(again, x, y),
                "{what} ({x}, {y})"
            );
        }
        free(again);
        free(out);
    }
    free(doc);
}
