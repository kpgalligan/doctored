//! Stamp Selected and Merge Group on a CMYK document (#547 phase 4c): the
//! stamped or merged leaf's straight samples against
//! `common/cmyk_structure_fixture.rs`'s f64 oracle (W3C on `L = 1 − ink`)
//! over the members' union frame, within 2/65535 on paper — the bound fixed
//! here before anything was measured — the placement, names and kept
//! properties, the composite unchanged by Merge Group for a Normal and a
//! Multiply isolated group, Pass Through landing Normal, every refusal by
//! its sentence and every plan admitted.

use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};

use crate::cmyk_structure_fixture::{
    assert_flat, cmyk_layers, composite, free, json, level, plan, ramp, refusal, then, try_edit,
    unit, varied, with_mask, with_mode, with_opacity, RgbLayer,
};

const W: u32 = 8;
const H: u32 = 6;
/// The bound, per plate on paper and on alpha, in 65535ths.
const BOUND: u16 = 2;

/// Bottom (opaque, canvas), A (6 × 5 at (−2, −1)), B (6 × 5 at (4, 3)), C
/// and D (canvas): A and B reach past the canvas.
fn five() -> *mut PrecisionDocument {
    let mut bottom = varied(W, H, 11);
    for p in &mut bottom {
        p[3] = 65535;
    }
    let layer = |name, w, h, offset, seed| RgbLayer {
        name,
        w,
        h,
        offset,
        pixels: varied(w, h, seed),
    };
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
            layer("A", 6, 5, (-2, -1), 12),
            layer("B", 6, 5, (4, 3), 13),
            layer("C", W, H, (0, 0), 14),
            layer("D", W, H, (0, 0), 15),
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

fn names(doc: *const PrecisionDocument) -> Vec<String> {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().name().to_string())
        .collect()
}

/// The leaf `name` against the oracle of `members` over its own frame.
fn assert_leaf(
    after: *const PrecisionDocument,
    name: &str,
    members: &[PrecisionDocumentLayer],
    frame: ((i32, i32), (u32, u32)),
) {
    let leaf = record(after, name);
    let image = leaf.cmyk_source().expect("a CMYK leaf");
    assert_eq!((leaf.offset(), image.dimensions()), frame, "{name}'s frame");
    let ((x0, y0), (w, h)) = frame;
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (i64::from(x0) + i64::from(x), i64::from(y0) + i64::from(y));
            let mut want = [0.0; 5];
            level(members, cx, cy, &mut want);
            assert_flat(
                image.sample_at(x, y).unwrap(),
                want,
                BOUND,
                &format!("{name} at ({cx}, {cy})"),
            );
        }
    }
}

fn assert_unchanged(before: *const PrecisionDocument, after: *const PrecisionDocument, what: &str) {
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            assert_flat(
                composite(after, x, y),
                unit(composite(before, x, y)),
                BOUND,
                &format!("{what} at ({x}, {y})"),
            );
        }
    }
}

fn edit(doc: *const PrecisionDocument, request: &str) -> *mut PrecisionDocument {
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
fn stamp_selected_composites_its_members_into_a_new_leaf_above_the_topmost() {
    let mut doc = five();
    doc = with_mode(doc, 2, BlendMode::Multiply.to_c());
    doc = with_opacity(doc, 2, 0.6);
    doc = with_mask(doc, 3, &ramp(W, H, 5));
    let members = [record(doc, "A"), record(doc, "B"), record(doc, "C")];
    let before_samples: Vec<_> = (0..5)
        .map(|i| unsafe { &*doc }.cmyk_layer_samples(i).unwrap())
        .collect();
    let after = edit(
        doc,
        r#"{"op":"stamp_selected","layers":[3,1,2,2],"name":"C (merged)"}"#,
    );
    assert_eq!(
        names(after),
        ["Bottom", "A", "B", "C", "C (merged)", "D"],
        "the stamp lands directly above the topmost member"
    );
    // A ∪ B ∪ C = (−2, −1)…(10, 8).
    assert_leaf(after, "C (merged)", &members, ((-2, -1), (12, 9)));
    let leaf = record(after, "C (merged)");
    assert_eq!(leaf.blend_mode(), BlendMode::Normal);
    assert_eq!(leaf.opacity(), 1.0);
    assert!(!leaf.clipped() && leaf.visible() && leaf.mask().is_none());
    assert!(members.iter().all(|m| m.id() != leaf.id()), "a new id");
    // The originals are untouched.
    for (i, name) in ["Bottom", "A", "B", "C"].iter().enumerate() {
        assert_eq!(
            unsafe { &*after }
                .cmyk_layer_samples(index(after, name))
                .unwrap(),
            before_samples[i],
            "{name} untouched"
        );
    }
    assert_eq!(record(after, "B").blend_mode(), BlendMode::Multiply);
    free(after);
    free(doc);
}

#[test]
fn stamp_selected_takes_one_group_with_its_own_appearance() {
    let mut doc = five();
    doc = then(
        doc,
        r#"{"op":"group_layers","layers":[1,2],"name":"G","pass_through":false}"#,
        &[],
    );
    let g = index(doc, "G");
    doc = with_opacity(doc, g, 0.7);
    doc = with_mode(doc, g, BlendMode::Multiply.to_c());
    let members = [record(doc, "G")];
    let after = edit(
        doc,
        &format!(r#"{{"op":"stamp_selected","layers":[{g}],"name":"G (merged)"}}"#),
    );
    assert_eq!(
        names(after),
        ["Bottom", "A", "B", "G", "G (merged)", "C", "D"]
    );
    assert_leaf(after, "G (merged)", &members, ((-2, -1), (12, 9)));
    free(after);
    free(doc);
}

#[test]
fn merge_group_keeps_the_composite_of_an_isolated_group() {
    for mode in [BlendMode::Normal, BlendMode::Multiply] {
        let mut doc = five();
        doc = with_mode(doc, 2, BlendMode::Multiply.to_c());
        doc = with_opacity(doc, 2, 0.6);
        doc = then(
            doc,
            r#"{"op":"group_layers","layers":[1,2],"name":"G","pass_through":false}"#,
            &[],
        );
        let g = index(doc, "G");
        doc = with_opacity(doc, g, 0.7);
        if mode != BlendMode::Normal {
            doc = with_mode(doc, g, mode.to_c());
        }
        doc = with_mask(doc, g, &ramp(W, H, 21));
        doc = then(
            doc,
            &format!(r#"{{"op":"layer_label","layer":{g},"label":3}}"#),
            &[],
        );
        let group = record(doc, "G");
        let children: Vec<PrecisionDocumentLayer> = group.children().unwrap().to_vec();
        let after = edit(doc, &format!(r#"{{"op":"merge_group","layer":{g}}}"#));
        assert_eq!(names(after), ["Bottom", "G", "C", "D"], "{mode:?}");
        // The RAW group: its children alone, over their union.
        assert_leaf(after, "G", &children, ((-2, -1), (12, 9)));
        let leaf = record(after, "G");
        assert!(leaf.children().is_none());
        assert_eq!(leaf.id(), group.id());
        assert_eq!(leaf.label(), 3);
        assert_eq!(leaf.opacity(), 0.7);
        assert_eq!(leaf.blend_mode(), mode);
        assert!(leaf.mask().is_some(), "the group's mask is kept");
        assert_unchanged(doc, after, &format!("merge group {mode:?}"));
        free(after);
        free(doc);
    }
}

#[test]
fn merge_group_lands_a_pass_through_group_normal() {
    let mut doc = five();
    doc = then(
        doc,
        r#"{"op":"group_layers","layers":[1,2],"name":"P","pass_through":true}"#,
        &[],
    );
    let g = index(doc, "P");
    let after = edit(doc, &format!(r#"{{"op":"merge_group","layer":{g}}}"#));
    let leaf = record(after, "P");
    assert_eq!(leaf.blend_mode(), BlendMode::Normal);
    assert!(!leaf.is_pass_through() && leaf.children().is_none());
    free(after);
    free(doc);
}

#[test]
fn stamp_selected_and_merge_group_refuse_by_name() {
    let mut doc = five();
    doc = then(
        doc,
        r#"{"op":"group_layers","layers":[3,4],"name":"G","pass_through":false}"#,
        &[],
    );
    // [Bottom, A, B, C, D, G]
    assert_eq!(
        refusal(
            doc,
            r#"{"op":"stamp_selected","layers":[1],"name":"S"}"#,
            &[]
        ),
        "Stamp Selected requires a group or at least two layers; layer 1 is not a group"
    );
    assert_eq!(
        refusal(
            doc,
            r#"{"op":"stamp_selected","layers":[1,3],"name":"S"}"#,
            &[]
        ),
        "Stamp Selected requires layers that share one parent"
    );
    assert_eq!(
        refusal(
            doc,
            r#"{"op":"stamp_selected","layers":[],"name":"S"}"#,
            &[]
        ),
        "Stamp Selected requires at least one layer"
    );
    assert_eq!(
        refusal(doc, r#"{"op":"merge_group","layer":1}"#, &[]),
        "Merge Group requires a visible group; layer 1 is not one"
    );
    let hidden = then(
        doc,
        r#"{"op":"layer_visible","layer":5,"visible":false}"#,
        &[],
    );
    assert_eq!(
        refusal(hidden, r#"{"op":"merge_group","layer":5}"#, &[]),
        "Merge Group requires a visible group; layer 5 is not one"
    );
    let shown = then(
        hidden,
        r#"{"op":"layer_visible","layer":5,"visible":true}"#,
        &[],
    );
    let locked = then(shown, r#"{"op":"layer_lock","layer":5,"locks":7}"#, &[]);
    let sentence = refusal(locked, r#"{"op":"merge_group","layer":5}"#, &[]);
    assert!(sentence.contains("lock"), "{sentence}");
    // Stamp Selected is lock-exempt.
    let stamped = edit(
        locked,
        r#"{"op":"stamp_selected","layers":[5],"name":"G (merged)"}"#,
    );
    free(stamped);
    free(locked);
}

#[test]
fn the_descriptor_carries_both_plans() {
    let mut doc = five();
    doc = then(
        doc,
        r#"{"op":"group_layers","layers":[1,2],"name":"G","pass_through":false}"#,
        &[],
    );
    let mut err = ptr::null_mut();
    let descriptor = json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err);
    for key in [
        "cmyk_stamp_selected_scratch_bytes",
        "cmyk_merge_group_scratch_bytes",
    ] {
        assert!(
            descriptor[key].as_u64().is_some_and(|v| v > 0),
            "{key}: {descriptor}"
        );
    }
    free(doc);
}
