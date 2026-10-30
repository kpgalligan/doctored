//! LAYER COMPS follow whole-canvas GEOMETRY at both depths.
//!
//! The rule (`comps_geometry`'s module doc) is a COMMUTATION: for every
//! whole-canvas op, applying a comp after the op must leave every layer
//! exactly where applying it before the op and then running the op leaves
//! it — offsets and style JSON alike. That is checked here over every
//! whole-canvas `GeometryEdit` (an exhaustive `match`, so a new variant has
//! to be classified before this file compiles) and over the byte exports
//! the host calls directly, against a fixture whose comp was saved and whose
//! rasters were then MOVED, so a remap that did nothing would fail.
//!
//! Beside it, hand-computed oracles for four ops, the false "locked" badge,
//! the recorded style under Image Size and under a SCALING straighten, the
//! entries that must NOT change, the purity rule, the per-layer ops that
//! leave comps alone, a two-op chain's ±1 px guarantee and a `.rz` round
//! trip.

use std::collections::{BTreeMap, HashSet};
use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_comps::LayerComp;
use doctored_core::ffi_comps::{dr_doc_edit_layer_comps, dr_doc_layer_comps_json};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_geometry::dr_doc_geometry_edit;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::{affine_set, cpath, ffi_style, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 40;
const H: u32 = 30;
/// `DR_LOCK_POSITION`, restated rather than read out of the core.
const LOCK_POSITION: u32 = 4;
/// `DrResizeFilter`'s bilinear code.
const BILINEAR: i32 = 1;
/// Two effects whose pixel sizes Scale Effects rewrites.
const STYLE: &str = r##"{"effects":[{"type":"drop_shadow","distance":3,"size":4},
    {"type":"stroke","size":2,"color":"#FF0000"}]}"##;
/// A straighten matrix that SCALES: det = 1.0025, so Scale Effects runs.
const SCALING: [f64; 6] = [1.0, 0.05, -0.05, 1.0, 0.5, -0.25];

/// Where the fixture's rasters sit when the comp is saved, and how far each
/// is then moved.
const WIDE_AT: (i32, i32) = (3, 5);
const WIDE_DIMS: (i32, i32) = (7, 4);
const TALL_AT: (i32, i32) = (20, 10);
const STYLED_AT: (i32, i32) = (10, 12);
const MOVES: [(&str, i32, i32); 3] = [("Wide", 2, -1), ("Tall", -3, 4), ("Styled", 1, 1)];

// ------------------------------------------------------------- plumbing --

fn owned(out: *mut DrDocument) -> Option<DrDocument> {
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn owned_typed(out: *mut PrecisionDocument) -> Option<PrecisionDocument> {
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn save_request() -> Value {
    json!({"op": "layer_comp", "action": "save", "name": "Shot"})
}

fn apply_request() -> Value {
    json!({"op": "layer_comp", "action": "apply", "comp": 0})
}

/// One comp request through `dr_doc_edit_layer_comps`. `Ok(None)` is the
/// exact no-op (NULL with no message).
fn byte_comp(doc: &DrDocument, request: &Value) -> Result<Option<DrDocument>, String> {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, c.as_ptr(), &mut err) };
    if out.is_null() {
        return if err.is_null() {
            Ok(None)
        } else {
            Err(take_err_string(err))
        };
    }
    Ok(owned(out))
}

fn byte_applied(doc: DrDocument) -> DrDocument {
    match byte_comp(&doc, &apply_request()).expect("apply is admitted") {
        Some(next) => next,
        None => doc,
    }
}

fn byte_report(doc: &DrDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_layer_comps_json(doc, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).expect("comp JSON")
}

/// One typed request through `dr_precision_doc_edit`.
fn typed_edit(
    doc: &PrecisionDocument,
    request: &Value,
) -> Result<Option<PrecisionDocument>, String> {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return if err.is_null() {
            Ok(None)
        } else {
            Err(take_err_string(err))
        };
    }
    Ok(owned_typed(out))
}

fn typed_must(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
    match typed_edit(doc, request) {
        Ok(Some(next)) => next,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn typed_applied(doc: PrecisionDocument) -> PrecisionDocument {
    match typed_edit(&doc, &apply_request()).expect("apply is admitted") {
        Some(next) => next,
        None => doc,
    }
}

fn typed_report(doc: &PrecisionDocument) -> Value {
    serde_json::from_str(&doc.layer_comps_json().expect("comp JSON")).expect("comp JSON")
}

/// What a comp can put back, per layer IDENTITY.
#[derive(Debug, PartialEq)]
struct State {
    offset: (i32, i32),
    style: Option<String>,
    visible: bool,
    opacity: f32,
}

fn byte_state(doc: &DrDocument) -> BTreeMap<u64, State> {
    (0..unsafe { dr_doc_layer_count(doc) })
        .map(|i| unsafe {
            (
                doc.layers[i].id,
                State {
                    offset: (dr_doc_layer_offset_x(doc, i), dr_doc_layer_offset_y(doc, i)),
                    style: ffi_style(doc, i),
                    visible: dr_doc_layer_visible(doc, i),
                    opacity: dr_doc_layer_opacity(doc, i),
                },
            )
        })
        .collect()
}

fn typed_state(doc: &PrecisionDocument) -> BTreeMap<u64, State> {
    (0..doc.layer_count())
        .map(|i| {
            let layer = doc.layer(i).expect("a record");
            (
                layer.id(),
                State {
                    offset: layer.offset(),
                    style: layer.reported_style(),
                    visible: layer.visible(),
                    opacity: layer.opacity(),
                },
            )
        })
        .collect()
}

fn byte_named(doc: &DrDocument, name: &str) -> usize {
    (0..doc.layers.len())
        .find(|&i| doc.layers[i].name == name)
        .unwrap_or_else(|| panic!("no layer named {name}"))
}

fn typed_named(doc: &PrecisionDocument, name: &str) -> usize {
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record named {name}"))
}

/// The comp entry recorded for layer `id`.
fn entry_offset(comp: &LayerComp, id: u64) -> Option<(i32, i32)> {
    comp.entries
        .iter()
        .find(|e| e.layer_id == id)
        .expect("an entry")
        .offset
}

// -------------------------------------------------------------- fixtures --

fn pattern(w: u32, h: u32, seed: u8) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            seed.wrapping_add((x * 37 + y * 11) as u8),
            seed.wrapping_add((x * 5 + y * 53) as u8),
            seed,
            255,
        ])
    })
}

/// Base (canvas), Wide 7×4, Tall 3×6 inside a group, Styled 5×5 carrying a
/// drop shadow and a stroke, and a byte ADJUSTMENT layer (Tone); a comp is
/// saved with everything at rest.
fn byte_saved() -> DrDocument {
    let mut doc = DrDocument::from_pixels(pattern(W, H, 9));
    doc.layers[0].name = "Base".into();
    doc = doc
        .adding_image_layer(0, pattern(7, 4, 40), "Wide")
        .unwrap();
    doc.layers[1].offset = WIDE_AT;
    doc = doc
        .adding_image_layer(1, pattern(3, 6, 80), "Tall")
        .unwrap();
    doc.layers[2].offset = TALL_AT;
    doc = doc
        .adding_image_layer(2, pattern(5, 5, 120), "Styled")
        .unwrap();
    doc.layers[3].offset = STYLED_AT;
    doc = doc
        .set_layer_style(3, Some(Arc::new(LayerStyle::from_json(STYLE).unwrap())))
        .unwrap();
    doc = doc
        .adding_image_layer(3, RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0])), "Tone")
        .unwrap();
    doc.layers[4].meta = Some(json!({"type":"adjust","op":"invert","params":{}}).to_string());
    let (doc, _, _, _) = doc.group_layers(&[2], "Group").expect("the group");
    byte_comp(&doc, &save_request())
        .expect("the comp saves")
        .expect("a change")
}

/// `byte_saved` with every raster — and the adjustment — moved away from
/// where the comp recorded it.
fn byte_moved() -> DrDocument {
    let mut doc = byte_saved();
    for (name, dx, dy) in MOVES {
        let i = byte_named(&doc, name);
        let (x, y) = doc.layers[i].offset;
        doc = owned(unsafe { dr_doc_with_layer_offset(&doc, i, x + dx, y + dy) }).unwrap();
    }
    let tone = byte_named(&doc, "Tone");
    owned(unsafe { dr_doc_with_layer_offset(&doc, tone, 2, 1) }).unwrap()
}

fn image16(w: u32, h: u32, seed: u16) -> PrecisionImage {
    let samples = (0..w * h)
        .map(|i| [seed + (i as u16) * 13, 20000 + (i as u16) * 7, 30000, 65535])
        .collect();
    PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap()
}

/// The typed twin: Base, Wide, a group around Tall, Styled — postorder
/// indices 0, 1, 2 (Tall), 3 (Group), 4 (Styled). A typed adjustment records
/// no offset, so the adjustment rows are the byte depth's.
fn typed_saved() -> PrecisionDocument {
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Base", image16(W, H, 1000), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Wide", image16(7, 4, 2000), WIDE_AT, 1.0, true).unwrap(),
            Layer::isolated_normal(
                "Group",
                vec![Layer::raster("Tall", image16(3, 6, 3000), TALL_AT, 1.0, true).unwrap()],
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster("Styled", image16(5, 5, 4000), STYLED_AT, 1.0, true).unwrap(),
        ],
    )
    .unwrap();
    let style: Value = serde_json::from_str(STYLE).unwrap();
    let styled = typed_must(
        &doc,
        &json!({"op":"layer_style","version":1,"layer":4,"style":style}),
    );
    typed_must(&styled, &save_request())
}

fn translate(layer: usize, dx: i32, dy: i32) -> Value {
    json!({"op":"affine_set","layers":[layer],"matrix":[1,0,0,1,dx,dy],"filter":"nearest"})
}

fn typed_moved() -> PrecisionDocument {
    let mut doc = typed_saved();
    for (name, dx, dy) in MOVES {
        let i = typed_named(&doc, name);
        doc = typed_must(&doc, &translate(i, dx, dy));
    }
    doc
}

// ------------------------------------------------------- the edit table --

/// Whether a whole-canvas edit scales layer styles; `None` for a PER-LAYER
/// edit, which leaves comps alone. Exhaustive with no `_` arm, so a new
/// `GeometryEdit` variant must be classified here before this file builds.
fn scales(edit: &GeometryEdit) -> Option<bool> {
    match edit {
        GeometryEdit::Resize { .. } => Some(true),
        GeometryEdit::Straighten { matrix, .. } => {
            Some(((matrix[0] * matrix[3] - matrix[1] * matrix[2]).abs() - 1.0).abs() > 1e-6)
        }
        GeometryEdit::Crop { .. }
        | GeometryEdit::CanvasSize { .. }
        | GeometryEdit::RotateCanvas { .. }
        | GeometryEdit::Rotate90
        | GeometryEdit::Rotate180
        | GeometryEdit::Rotate270
        | GeometryEdit::FlipHorizontal
        | GeometryEdit::FlipVertical => Some(false),
        GeometryEdit::Affine { .. }
        | GeometryEdit::Perspective { .. }
        | GeometryEdit::AffineSet { .. }
        | GeometryEdit::PerspectiveSet { .. } => None,
    }
}

/// One request per whole-canvas `GeometryEdit` variant.
fn whole_requests() -> Vec<Value> {
    vec![
        json!({"op":"resize","width":53,"height":37,"filter":"bilinear"}),
        json!({"op":"crop","x":5,"y":3,"width":28,"height":22}),
        json!({"op":"canvas_size","width":52,"height":40,"x":6,"y":-4}),
        json!({"op":"straighten","matrix":SCALING,"filter":"bilinear",
               "x":2,"y":2,"width":34,"height":24}),
        json!({"op":"rotate_canvas","degrees":17.0,"filter":"bilinear"}),
        json!({"op":"rotate90"}),
        json!({"op":"rotate180"}),
        json!({"op":"rotate270"}),
        json!({"op":"flip_horizontal"}),
        json!({"op":"flip_vertical"}),
    ]
}

fn parsed(request: &Value) -> GeometryEdit {
    GeometryEdit::parse(&request.to_string()).unwrap()
}

#[test]
fn the_table_covers_every_whole_canvas_variant() {
    let kinds: HashSet<_> = whole_requests()
        .iter()
        .map(|request| {
            let edit = parsed(request);
            assert!(scales(&edit).is_some(), "{request} is whole-canvas");
            std::mem::discriminant(&edit)
        })
        .collect();
    assert_eq!(kinds.len(), 10, "one row per whole-canvas variant");
}

fn byte_geometry(doc: &DrDocument, request: &Value) -> DrDocument {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_geometry_edit(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    owned(out).unwrap()
}

type ByteOp = Box<dyn Fn(&DrDocument) -> DrDocument>;

/// Every byte whole-canvas path: `dr_doc_geometry_edit` per variant plus the
/// exports the host calls directly. `(name, scales styles, op)`.
fn byte_ops() -> Vec<(String, bool, ByteOp)> {
    let mut ops: Vec<(String, bool, ByteOp)> = whole_requests()
        .into_iter()
        .map(|request| {
            let scaling = scales(&parsed(&request)).unwrap();
            let name = format!("dr_doc_geometry_edit {request}");
            let op: ByteOp = Box::new(move |d| byte_geometry(d, &request));
            (name, scaling, op)
        })
        .collect();
    let direct: Vec<(&str, bool, ByteOp)> = vec![
        (
            "dr_doc_crop",
            false,
            Box::new(|d| owned(unsafe { dr_doc_crop(d, 5, 3, 28, 22) }).unwrap()),
        ),
        (
            "dr_doc_canvas_resize grow",
            false,
            Box::new(|d| owned(unsafe { dr_doc_canvas_resize(d, 52, 40, 6, 4) }).unwrap()),
        ),
        (
            "dr_doc_canvas_resize shrink",
            false,
            Box::new(|d| owned(unsafe { dr_doc_canvas_resize(d, 32, 24, -2, -1) }).unwrap()),
        ),
        (
            "dr_doc_canvas_resize negative origin",
            false,
            Box::new(|d| owned(unsafe { dr_doc_canvas_resize(d, 44, 34, -3, -2) }).unwrap()),
        ),
        (
            "dr_doc_resize",
            true,
            Box::new(|d| owned(unsafe { dr_doc_resize(d, 53, 37, BILINEAR) }).unwrap()),
        ),
        (
            "dr_doc_rotate90",
            false,
            Box::new(|d| owned(unsafe { dr_doc_rotate90(d) }).unwrap()),
        ),
        (
            "dr_doc_rotate180",
            false,
            Box::new(|d| owned(unsafe { dr_doc_rotate180(d) }).unwrap()),
        ),
        (
            "dr_doc_rotate270",
            false,
            Box::new(|d| owned(unsafe { dr_doc_rotate270(d) }).unwrap()),
        ),
        (
            "dr_doc_flip_horizontal",
            false,
            Box::new(|d| owned(unsafe { dr_doc_flip_horizontal(d) }).unwrap()),
        ),
        (
            "dr_doc_flip_vertical",
            false,
            Box::new(|d| owned(unsafe { dr_doc_flip_vertical(d) }).unwrap()),
        ),
    ];
    ops.extend(
        direct
            .into_iter()
            .map(|(name, scaling, op)| (name.to_string(), scaling, op)),
    );
    ops
}

/// The fields a geometry op must never touch, and — when it does not scale
/// styles — the recorded style too.
fn assert_untouched_fields(name: &str, before: &LayerComp, after: &LayerComp, scaling: bool) {
    assert_eq!(
        (&before.name, &before.comment, before.captures),
        (&after.name, &after.comment, after.captures),
        "{name}"
    );
    assert_eq!(before.entries.len(), after.entries.len(), "{name}");
    for (old, new) in before.entries.iter().zip(&after.entries) {
        assert_eq!(old.layer_id, new.layer_id, "{name}");
        assert_eq!(old.layer_name, new.layer_name, "{name}");
        assert_eq!(old.visible, new.visible, "{name}: visibility");
        assert_eq!(old.offset.is_some(), new.offset.is_some(), "{name}");
        let (Some(a), Some(b)) = (&old.appearance, &new.appearance) else {
            panic!("{name}: the comp captures appearance");
        };
        assert_eq!((a.opacity, a.blend), (b.opacity, b.blend), "{name}");
        if !scaling {
            assert_eq!(a.style, b.style, "{name}: the style is byte-identical");
        }
    }
}

// --------------------------------------------------------- commutation --

#[test]
fn byte_op_then_apply_equals_apply_then_op_for_every_whole_canvas_path() {
    let moved = byte_moved();
    for (name, scaling, op) in byte_ops() {
        let done = op(&moved);
        let (before, after) = (&moved.layer_comps()[0], &done.layer_comps()[0]);
        assert_untouched_fields(&name, before, after, scaling);

        let op_then_apply = byte_state(&byte_applied(done.clone()));
        let apply_then_op = byte_state(&op(&byte_applied(moved.clone())));
        assert_eq!(op_then_apply, apply_then_op, "{name}");

        // The recorded style IS what the op makes of the live one — scaled
        // under Image Size and the scaling straighten, byte-identical
        // otherwise (checked above).
        for entry in &after.entries {
            let style = &entry.appearance.as_ref().unwrap().style;
            assert_eq!(style, &apply_then_op[&entry.layer_id].style, "{name}");
        }
        if scaling {
            let styled = done.layers[byte_named(&done, "Styled")].id;
            let recorded = |comp: &LayerComp| {
                comp.entries
                    .iter()
                    .find(|e| e.layer_id == styled)
                    .and_then(|e| e.appearance.as_ref()?.style.clone())
            };
            assert_ne!(
                recorded(before),
                recorded(after),
                "{name}: the style scaled"
            );
        }
    }
}

#[test]
fn typed_op_then_apply_equals_apply_then_op_for_every_whole_canvas_edit() {
    let moved = typed_moved();
    for request in whole_requests() {
        let scaling = scales(&parsed(&request)).unwrap();
        let name = format!("dr_precision_doc_edit {request}");
        let done = typed_must(&moved, &request);
        let (before, after) = (&moved.layer_comps()[0], &done.layer_comps()[0]);
        assert_untouched_fields(&name, before, after, scaling);

        let op_then_apply = typed_state(&typed_applied(done.clone()));
        let apply_then_op = typed_state(&typed_must(&typed_applied(moved.clone()), &request));
        assert_eq!(op_then_apply, apply_then_op, "{name}");
        for entry in &after.entries {
            let style = &entry.appearance.as_ref().unwrap().style;
            assert_eq!(style, &apply_then_op[&entry.layer_id].style, "{name}");
        }
    }
}

// --------------------------------------------------- hand-computed rules --

#[test]
fn four_ops_move_the_recorded_offset_by_their_hand_computed_rule() {
    let (rx, ry) = WIDE_AT;
    let (lw, lh) = WIDE_DIMS;
    let (cw, ch) = (W as i32, H as i32);
    let cases: [(&str, Value, (i32, i32)); 4] = [
        (
            "crop",
            json!({"op":"crop","x":5,"y":3,"width":28,"height":22}),
            (rx - 5, ry - 3),
        ),
        ("rotate90", json!({"op":"rotate90"}), (ch - ry - lh, rx)),
        (
            "flip_horizontal",
            json!({"op":"flip_horizontal"}),
            (cw - rx - lw, ry),
        ),
        (
            "resize",
            json!({"op":"resize","width":53,"height":37,"filter":"bilinear"}),
            (
                (f64::from(rx) * 53.0 / 40.0).round() as i32,
                (f64::from(ry) * 37.0 / 30.0).round() as i32,
            ),
        ),
    ];
    let byte = byte_saved();
    let typed = typed_saved();
    let byte_wide = byte.layers[byte_named(&byte, "Wide")].id;
    let typed_wide = typed.layer(typed_named(&typed, "Wide")).unwrap().id();
    for (name, request, expected) in cases {
        let done = byte_geometry(&byte, &request);
        assert_eq!(
            entry_offset(&done.layer_comps()[0], byte_wide),
            Some(expected),
            "byte {name}"
        );
        let done = typed_must(&typed, &request);
        assert_eq!(
            entry_offset(&done.layer_comps()[0], typed_wide),
            Some(expected),
            "typed {name}"
        );
    }
    // (21, 3), (30, 5) and (4, 6) written out once, so the table above is
    // not only checked against itself.
    assert_eq!((ch - ry - lh, rx), (21, 3));
    assert_eq!((cw - rx - lw, ry), (30, 5));
    assert_eq!(
        (
            (f64::from(rx) * 53.0 / 40.0).round() as i32,
            (f64::from(ry) * 37.0 / 30.0).round() as i32
        ),
        (4, 6)
    );
}

// ------------------------------------------------------ the locked badge --

#[test]
fn a_crop_does_not_report_a_position_locked_entry_locked_at_either_depth() {
    let doc = byte_saved();
    let wide = byte_named(&doc, "Wide");
    let locked = owned(unsafe {
        doctored_core::ffi_group::dr_doc_with_layer_locks(&doc, wide, LOCK_POSITION)
    })
    .unwrap();
    let saved = byte_comp(
        &locked,
        &json!({"op":"layer_comp","action":"save","name":"Locked"}),
    )
    .unwrap()
    .unwrap();
    let cropped = owned(unsafe { dr_doc_crop(&saved, 5, 3, 28, 22) }).unwrap();
    let report = byte_report(&cropped);
    for comp in report["comps"].as_array().unwrap() {
        assert_eq!(comp["locked_count"], 0, "{comp}");
        assert!(comp["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["locked"] == json!(false)));
    }
    assert!(
        byte_comp(
            &cropped,
            &json!({"op":"layer_comp","action":"apply","comp":1})
        )
        .expect("no error")
        .is_none(),
        "applying the comp after the crop is an exact no-op"
    );

    let doc = typed_saved();
    let wide = typed_named(&doc, "Wide");
    let locked = typed_must(
        &doc,
        &json!({"op":"layer_lock","layer":wide,"locks":LOCK_POSITION}),
    );
    let saved = typed_must(
        &locked,
        &json!({"op":"layer_comp","action":"save","name":"Locked"}),
    );
    let cropped = typed_must(
        &saved,
        &json!({"op":"crop","x":5,"y":3,"width":28,"height":22}),
    );
    let report = typed_report(&cropped);
    for comp in report["comps"].as_array().unwrap() {
        assert_eq!(comp["locked_count"], 0, "{comp}");
        assert!(comp["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["locked"] == json!(false)));
    }
    assert!(
        typed_edit(
            &cropped,
            &json!({"op":"layer_comp","action":"apply","comp":1})
        )
        .expect("no error")
        .is_none(),
        "applying the typed comp after the crop is an exact no-op"
    );
}

// ------------------------------------------------ what must NOT change --

#[test]
fn stale_group_position_less_and_straightened_adjustment_entries_are_untouched() {
    let saved = byte_saved();
    let wide_id = saved.layers[byte_named(&saved, "Wide")].id;
    let group_id = saved.layers[byte_named(&saved, "Group")].id;
    let tone_id = saved.layers[byte_named(&saved, "Tone")].id;
    let no_position = byte_comp(
        &saved,
        &json!({"op":"layer_comp","action":"save","name":"No position","position":false}),
    )
    .unwrap()
    .unwrap();
    // A STALE entry: its layer is gone, so nothing can say where it went.
    let stale =
        owned(unsafe { dr_doc_removing_layer(&no_position, byte_named(&saved, "Wide")) }).unwrap();
    let cropped = owned(unsafe { dr_doc_crop(&stale, 5, 3, 28, 22) }).unwrap();
    let (before, after) = (&stale.layer_comps()[0], &cropped.layer_comps()[0]);
    assert_eq!(
        entry_offset(after, wide_id),
        entry_offset(before, wide_id),
        "a stale entry keeps its recorded offset"
    );
    assert_eq!(entry_offset(before, wide_id), Some(WIDE_AT));
    // A GROUP entry records no offset and gains none.
    assert_eq!(entry_offset(before, group_id), None);
    assert_eq!(entry_offset(after, group_id), None);
    // A comp that does not capture position is left exactly as it was.
    assert_eq!(stale.layer_comps()[1], cropped.layer_comps()[1]);
    assert!(cropped.layer_comps()[1]
        .entries
        .iter()
        .all(|e| e.offset.is_none()));

    // A byte ADJUSTMENT is not moved by a straighten, and neither is its
    // entry.
    let straightened = byte_geometry(
        &saved,
        &json!({"op": "straighten", "matrix": SCALING, "filter": "bilinear",
            "x": 0, "y": 0, "width": saved.width, "height": saved.height}),
    );
    let (before, after) = (&saved.layer_comps()[0], &straightened.layer_comps()[0]);
    let tone_before = before.entries.iter().find(|e| e.layer_id == tone_id);
    let tone_after = after.entries.iter().find(|e| e.layer_id == tone_id);
    assert_eq!(tone_before, tone_after, "the Tone entry is untouched");
    assert_eq!(
        straightened.layers[byte_named(&straightened, "Tone")].offset,
        (0, 0),
        "and so is the layer"
    );
}

#[test]
fn a_flip_that_changes_only_the_comps_is_an_edit_and_leaves_its_input_alone() {
    // Two SOLID layers, the small one centred, so a horizontal flip changes
    // no pixel and no offset. With a comp recorded off-centre, only the comp
    // moves — which is still an edit. The ruler origin sits at the canvas
    // centre too, because a flip mirrors it.
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([30, 60, 90, 255])))
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(4, 4, Rgba([200, 10, 10, 255])),
            "Small",
        )
        .unwrap()
        .set_ruler_origin(f64::from(W) / 2.0, f64::from(H) / 2.0)
        .unwrap();
    let doc = owned(unsafe { dr_doc_with_layer_offset(&base, 1, 5, 5) }).unwrap();
    let doc = byte_comp(&doc, &save_request()).unwrap().unwrap();
    let centred = owned(unsafe { dr_doc_with_layer_offset(&doc, 1, 18, 13) }).unwrap();
    let small = centred.layers[1].id;

    let recorded = centred.layer_comps().to_vec();
    let flipped = owned(unsafe { dr_doc_flip_horizontal(&centred) })
        .expect("a flip that moves only a comp still answers a document");
    assert_eq!(
        centred.layer_comps(),
        &recorded[..],
        "the input's comps are untouched"
    );
    assert_eq!(flipped.layers[1].offset, (18, 13), "the layer did not move");
    assert_eq!(
        entry_offset(&flipped.layer_comps()[0], small),
        Some((40 - 5 - 4, 5))
    );

    // The same document with no comp at all: the flip IS an exact no-op.
    let plain = owned(unsafe { dr_doc_with_layer_offset(&base, 1, 18, 13) }).unwrap();
    assert!(owned(unsafe { dr_doc_flip_horizontal(&plain) }).is_none());
}

#[test]
fn per_layer_transforms_leave_comps_alone() {
    let moved = byte_moved();
    let wide = byte_named(&moved, "Wide");
    let (out, message) = affine_set(&moved, &[wide], [1.2, 0.0, 0.0, 1.0, 3.0, 2.0]);
    assert!(message.is_none(), "{message:?}");
    let out = owned(out).expect("the transform is an edit");
    assert_eq!(out.layer_comps(), moved.layer_comps(), "byte affine_set");

    let moved = typed_moved();
    let wide = typed_named(&moved, "Wide");
    let out = typed_must(
        &moved,
        &json!({"op":"affine","layer":wide,"target":"pixels",
                "matrix":[1.2,0,0,1,3,2],"filter":"bilinear"}),
    );
    assert_eq!(out.layer_comps(), moved.layer_comps(), "typed affine");
}

// ------------------------------------------------------------ the chain --

#[test]
fn a_filtered_rotation_then_a_flip_drifts_by_at_most_one_pixel() {
    let rotate = json!({"op":"rotate_canvas","degrees":17.0,"filter":"bilinear"});
    let flip = json!({"op":"flip_horizontal"});
    let within_one = |name: &str, a: &BTreeMap<u64, State>, b: &BTreeMap<u64, State>| {
        assert_eq!(a.len(), b.len());
        for (id, left) in a {
            let right = &b[id];
            assert!(
                (left.offset.0 - right.offset.0).abs() <= 1
                    && (left.offset.1 - right.offset.1).abs() <= 1,
                "{name}: layer {id} {:?} vs {:?}",
                left.offset,
                right.offset
            );
        }
    };

    let moved = byte_moved();
    let chain = |d: &DrDocument| byte_geometry(&byte_geometry(d, &rotate), &flip);
    let op_then_apply = byte_state(&byte_applied(chain(&moved)));
    let apply_then_op = byte_state(&chain(&byte_applied(moved.clone())));
    within_one("byte", &op_then_apply, &apply_then_op);

    let moved = typed_moved();
    let chain = |d: &PrecisionDocument| typed_must(&typed_must(d, &rotate), &flip);
    let op_then_apply = typed_state(&typed_applied(chain(&moved)));
    let apply_then_op = typed_state(&chain(&typed_applied(moved.clone())));
    within_one("typed", &op_then_apply, &apply_then_op);
}

// ------------------------------------------------------------ persistence --

#[test]
fn a_remapped_comp_survives_save_and_reopen_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let expected = (WIDE_AT.0 - 5, WIDE_AT.1 - 3);

    let cropped = owned(unsafe { dr_doc_crop(&byte_moved(), 5, 3, 28, 22) }).unwrap();
    let path = dir.path().join("cropped.rz");
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(&cropped, c.as_ptr(), &mut err) },
        "{}",
        take_err_string(err)
    );
    let mut err: *mut c_char = ptr::null_mut();
    let reopened = owned(unsafe { dr_doc_open(c.as_ptr(), &mut err) })
        .unwrap_or_else(|| panic!("{}", take_err_string(err)));
    let applied = byte_applied(reopened);
    assert_eq!(
        applied.layers[byte_named(&applied, "Wide")].offset,
        expected
    );

    let cropped = typed_must(
        &typed_moved(),
        &json!({"op":"crop","x":5,"y":3,"width":28,"height":22}),
    );
    let bytes = cropped.encode_native(LIMIT, 0, None).expect("typed bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    let applied = typed_applied(reopened);
    assert_eq!(
        applied
            .layer(typed_named(&applied, "Wide"))
            .unwrap()
            .offset(),
        expected
    );
}
