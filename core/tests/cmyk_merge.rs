//! Merge Down, Merge Layers, Merge Visible and Stamp Visible on a CMYK
//! document (#1 phase 4a): the merged leaf's straight samples against
//! `common/cmyk_structure_fixture.rs`'s f64 oracle over the members' union
//! frame (pixels off the canvas kept), the kept identity, name and clipping,
//! the document composite unchanged, and every plan admitted.

use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};

use crate::cmyk_structure_fixture::{
    assert_flat, cmyk_layers, composite, free, json, level, oracle, plan, ramp, then, try_edit,
    unit, varied, with_mask, with_mode, with_opacity, RgbLayer,
};

const W: u32 = 8;
const H: u32 = 6;

/// Bottom (opaque, canvas), A (6 × 5 at (−2, −1)), B (6 × 5 at (4, 3)), C
/// and D (canvas) — A and B reach past the canvas.
fn five() -> *mut PrecisionDocument {
    let mut bottom = varied(W, H, 1);
    for p in &mut bottom {
        p[3] = 65535;
    }
    cmyk_layers(
        W,
        H,
        vec![
            RgbLayer {
                name: "Bottom",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: bottom,
            },
            RgbLayer {
                name: "A",
                w: 6,
                h: 5,
                offset: (-2, -1),
                pixels: varied(6, 5, 2),
            },
            RgbLayer {
                name: "B",
                w: 6,
                h: 5,
                offset: (4, 3),
                pixels: varied(6, 5, 3),
            },
            RgbLayer {
                name: "C",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: varied(W, H, 4),
            },
            RgbLayer {
                name: "D",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: varied(W, H, 5),
            },
        ],
    )
}

fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn record(doc: *const PrecisionDocument, name: &str) -> PrecisionDocumentLayer {
    unsafe { &*doc }.layer(index(doc, name)).unwrap().clone()
}

/// The merged leaf `name` against the oracle of `members` over its own
/// extent, which must be `frame`.
fn assert_merged(
    after: *const PrecisionDocument,
    name: &str,
    members: &[PrecisionDocumentLayer],
    frame: ((i32, i32), (u32, u32)),
) {
    let leaf = record(after, name);
    let image = leaf.cmyk_source().expect("a CMYK leaf");
    assert_eq!((leaf.offset(), image.dimensions()), frame, "{name}'s frame");
    assert_eq!(leaf.blend_mode(), BlendMode::Normal);
    assert_eq!(leaf.opacity(), 1.0);
    let ((x0, y0), (w, h)) = frame;
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (i64::from(x0) + i64::from(x), i64::from(y0) + i64::from(y));
            let mut want = [0.0; 5];
            level(members, cx, cy, &mut want);
            assert_flat(
                image.sample_at(x, y).unwrap(),
                want,
                1,
                &format!("{name} at ({cx}, {cy})"),
            );
        }
    }
}

/// Every canvas pixel of `after` against `before` within 2/65535.
fn assert_unchanged(before: *const PrecisionDocument, after: *const PrecisionDocument, what: &str) {
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            assert_flat(
                composite(after, x, y),
                unit(composite(before, x, y)),
                2,
                &format!("{what} at ({x}, {y})"),
            );
        }
    }
}

fn merge(doc: *const PrecisionDocument, request: &str) -> *mut PrecisionDocument {
    let planned = plan(doc, request).unwrap_or_else(|e| panic!("{request}: {e}"));
    assert!(
        planned["scratch_bytes"].as_u64().is_some_and(|v| v > 0),
        "{request}: {planned}"
    );
    let out = try_edit(doc, request, &[]).unwrap_or_else(|e| panic!("{request}: {e}"));
    assert!(!out.is_null(), "{request} changed nothing");
    out
}

#[test]
fn merge_down_bakes_a_clipped_upper_over_the_union_frame() {
    let mut doc = five();
    doc = then(
        doc,
        r#"{"op":"layer_clipped","layer":2,"clipped":true}"#,
        &[],
    );
    doc = with_mode(doc, 2, BlendMode::Multiply.to_c());
    doc = with_opacity(doc, 2, 0.8);
    doc = with_mask(doc, 1, &ramp(W, H, 3));
    let members = [record(doc, "A"), record(doc, "B")];
    let after = merge(doc, r#"{"op":"merge_down","layer":2}"#);
    assert_eq!(unsafe { &*after }.layer_count(), 4);
    // A ∪ B = (−2, −1)…(10, 8).
    assert_merged(after, "A", &members, ((-2, -1), (12, 9)));
    let merged = record(after, "A");
    assert_eq!(merged.id(), members[0].id());
    assert!(!merged.clipped());
    assert!(merged.mask().is_none());
    assert_unchanged(doc, after, "merge down");
    free(after);
    free(doc);
}

#[test]
fn merge_layers_takes_a_group_member() {
    let mut doc = five();
    doc = then(
        doc,
        r#"{"op":"group_layers","layers":[3,4],"name":"G","pass_through":false}"#,
        &[],
    );
    let g = index(doc, "G");
    doc = with_opacity(doc, g, 0.6);
    doc = with_mode(doc, index(doc, "D"), BlendMode::Screen.to_c());
    doc = then(
        doc,
        r#"{"op":"layer_clipped","layer":1,"clipped":true}"#,
        &[],
    );
    let (a, gi) = (index(doc, "A"), index(doc, "G"));
    let members = [record(doc, "A"), record(doc, "G")];
    let after = merge(
        doc,
        &format!(r#"{{"op":"merge_layers","layers":[{a},{gi}]}}"#),
    );
    // A ∪ the canvas = (−2, −1)…(8, 6); A, the lowest, keeps its clipping.
    let merged = record(after, "A");
    assert_eq!(merged.id(), members[0].id());
    assert!(merged.clipped());
    assert_eq!(unsafe { &*after }.layer_count(), 3);
    // The lowest member is baseless in the members' own level, so it
    // composites as if unclipped there.
    assert_merged(after, "A", &members, ((-2, -1), (10, 7)));
    free(after);
    free(doc);
}

#[test]
fn merge_visible_and_stamp_visible_take_the_whole_composite() {
    let mut doc = five();
    doc = then(
        doc,
        r#"{"op":"layer_clipped","layer":2,"clipped":true}"#,
        &[],
    );
    doc = with_mode(doc, 3, BlendMode::Darken.to_c());
    doc = with_mask(doc, 4, &ramp(W, H, 9));
    let all: Vec<PrecisionDocumentLayer> = unsafe { &*doc }.root_layers().to_vec();
    let bottom = record(doc, "Bottom");
    let merged = merge(doc, r#"{"op":"merge_visible"}"#);
    assert_eq!(unsafe { &*merged }.layer_count(), 1);
    assert_merged(merged, "Bottom", &all, ((0, 0), (W, H)));
    assert_eq!(record(merged, "Bottom").id(), bottom.id());
    assert_unchanged(doc, merged, "merge visible");
    let stamped = merge(doc, r#"{"op":"stamp_visible","name":"Stamp","anchor":4}"#);
    assert_eq!(unsafe { &*stamped }.layer_count(), 6);
    assert_eq!(index(stamped, "Stamp"), 5);
    assert_merged(stamped, "Stamp", &all, ((0, 0), (W, H)));
    for y in 0..H as i64 {
        for x in 0..W as i64 {
            let want = oracle(doc, x, y);
            assert_flat(composite(doc, x as i32, y as i32), want, 2, "the source");
        }
    }
    let mut err = ptr::null_mut();
    let descriptor = json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err);
    for key in [
        "cmyk_merge_scratch_bytes",
        "cmyk_stamp_scratch_bytes",
        "cmyk_merge_down_scratch_bytes",
    ] {
        assert!(
            descriptor[key].as_u64().is_some_and(|v| v > 0),
            "{key}: {descriptor}"
        );
    }
    for d in [merged, stamped, doc] {
        free(d);
    }
}
