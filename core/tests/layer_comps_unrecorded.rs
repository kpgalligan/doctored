//! Applying a layer comp HIDES every layer the comp has no record of,
//! unless it is a group, at any depth, whatever the comp captured and
//! whatever the layer's locks, as measured in Photoshop 26.10.0
//! (`scripts/photoshop-probe/layer_comps.py`, receipt
//! `photoshop/layer-comps-521/take1`).
//!
//! Every case runs at 8 bits (`dr_doc_*`) and at 16 bits
//! (`dr_precision_doc_edit` / `dr_precision_doc_*`). The oracle is the rule
//! itself, written here from the layer ids each document held when the comp
//! was saved: a layer whose id was not there is unrecorded, and it ends hidden
//! unless it is a group; every other layer ends where the comp put it.

use std::collections::BTreeSet;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{DrDocument, LayerKind};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::{
    dr_doc_edit_layer_comps, dr_doc_layer_comps_json, dr_precision_doc_layer_comps_json,
};
use doctored_core::ffi_doc::{
    dr_doc_duplicating_layer, dr_doc_open, dr_doc_removing_layer, dr_doc_save_native,
};
use doctored_core::ffi_group::{
    dr_doc_duplicate_to_new_document, dr_doc_group_layers, dr_doc_with_layer_locks,
};
use doctored_core::ffi_insert::dr_doc_adding_layer_at;
use doctored_core::ffi_layer_clipboard::{dr_doc_paste_layers, dr_precision_doc_paste_layers};
use doctored_core::ffi_precision::{
    dr_precision_doc_duplicate_layer, dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 16;
const H: u32 = 16;
/// `DrInsertPosition`, restated here rather than read out of the core.
const ABOVE: u32 = 0;
const TOP: u32 = 1;
const INTO_GROUP: u32 = 2;
/// Every lock bit (`LOCK_TRANSPARENCY | LOCK_PIXELS | LOCK_POSITION`).
const LOCK_ALL: u32 = 7;

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// One layer as the rule sees it.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    id: u64,
    name: String,
    group: bool,
    visible: bool,
}

fn ids(rows: &[Row]) -> BTreeSet<u64> {
    rows.iter().map(|r| r.id).collect()
}

fn by_name<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    let mut hits = rows.iter().filter(|r| r.name == name);
    let row = hits.next().unwrap_or_else(|| panic!("no layer {name}"));
    assert!(hits.next().is_none(), "two layers called {name}");
    row
}

fn index_of(rows: &[Row], name: &str) -> usize {
    let id = by_name(rows, name).id;
    rows.iter().position(|r| r.id == id).expect("an index")
}

/// The rule's oracle. `recorded` is the id set the comp was saved over and
/// `recorded_visible` the visibility each recorded layer must end with (the
/// comp's own row for it); every layer not in `recorded` that is not a group
/// must be hidden, and an unrecorded group keeps `before`'s visibility.
fn assert_rule(before: &[Row], after: &[Row], recorded_visible: &[(u64, bool)], what: &str) {
    assert_eq!(
        ids(before),
        ids(after),
        "{what}: apply adds or drops nothing"
    );
    for row in after {
        let was = before.iter().find(|r| r.id == row.id).expect("same ids");
        match recorded_visible.iter().find(|(id, _)| *id == row.id) {
            Some((_, visible)) => assert_eq!(row.visible, *visible, "{what}: {}", row.name),
            None if row.group => assert_eq!(row.visible, was.visible, "{what}: {}", row.name),
            None => assert!(!row.visible, "{what}: unrecorded {} is hidden", row.name),
        }
    }
}

/// The comp `comp`'s `unrecorded` rows as (layer, id, name).
fn unrecorded(report: &Value, comp: usize) -> Vec<(usize, u64, String)> {
    report["comps"][comp]["unrecorded"]
        .as_array()
        .expect("unrecorded")
        .iter()
        .map(|row| {
            (
                row["layer"].as_u64().expect("a layer") as usize,
                row["id"].as_u64().expect("an id"),
                row["name"].as_str().expect("a name").to_string(),
            )
        })
        .collect()
}

/// The `unrecorded` rows the rule predicts for `rows`, given the recorded ids.
fn expected_unrecorded(rows: &[Row], recorded: &BTreeSet<u64>) -> Vec<(usize, u64, String)> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| !r.group && !recorded.contains(&r.id))
        .map(|(i, r)| (i, r.id, r.name.clone()))
        .collect()
}

// ================================================================= 8 bits ==

fn rows8(doc: &DrDocument) -> Vec<Row> {
    doc.layers
        .iter()
        .map(|l| Row {
            id: l.id,
            name: l.name.clone(),
            group: l.kind == LayerKind::Group,
            visible: l.visible,
        })
        .collect()
}

fn take8(out: *mut DrDocument, what: &str) -> DrDocument {
    assert!(!out.is_null(), "{what}");
    *unsafe { Box::from_raw(out) }
}

/// One comp request; `None` is the core's exact no-op.
fn comp8(doc: &DrDocument, request: Value) -> Option<DrDocument> {
    let mut request = request;
    request["op"] = json!("layer_comp");
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, text.as_ptr(), &mut err) };
    assert_eq!(owned(err), None, "{request}");
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn save8(doc: &DrDocument, name: &str) -> DrDocument {
    comp8(doc, json!({"action":"save","name":name})).expect("a save")
}

fn apply8(doc: &DrDocument, comp: usize) -> Option<DrDocument> {
    comp8(doc, json!({"action":"apply","comp":comp}))
}

fn report8(doc: &DrDocument) -> Value {
    let mut err = ptr::null_mut();
    let json = owned(unsafe { dr_doc_layer_comps_json(doc, &mut err) });
    assert_eq!(owned(err), None);
    serde_json::from_str(&json.expect("a report")).expect("report JSON")
}

fn add8(doc: &DrDocument, anchor: usize, position: u32, name: &str) -> DrDocument {
    let c = CString::new(name).unwrap();
    take8(
        unsafe { dr_doc_adding_layer_at(doc, anchor, position, c.as_ptr()) },
        name,
    )
}

fn group8(doc: &DrDocument, idx: &[usize], name: &str) -> DrDocument {
    let c = CString::new(name).unwrap();
    let mut group = 0usize;
    let (mut cleared, mut reordered) = (0usize, 0usize);
    take8(
        unsafe {
            dr_doc_group_layers(
                doc,
                idx.as_ptr(),
                idx.len(),
                c.as_ptr(),
                &mut group,
                ptr::null_mut(),
                &mut cleared,
                ptr::null_mut(),
                &mut reordered,
                0,
            )
        },
        name,
    )
}

fn visible8(doc: &DrDocument, name: &str, visible: bool) -> DrDocument {
    let mut out = doc.clone();
    let i = index_of(&rows8(doc), name);
    out.layers[i].visible = visible;
    out
}

fn square(v: u8) -> RgbaImage {
    RgbaImage::from_pixel(4, 4, Rgba([v, v, v, 255]))
}

/// Background, A, B, and the group G holding C, bottom first.
fn base8() -> DrDocument {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([128, 128, 128, 255])))
        .adding_image_layer(0, square(10), "A")
        .and_then(|d| d.adding_image_layer(1, square(20), "B"))
        .and_then(|d| d.adding_image_layer(2, square(30), "C"))
        .expect("three layers");
    let mut doc = group8(&doc, &[3], "G");
    doc.layers[0].name = "Background".into();
    doc
}

/// Every recorded layer's visibility in `doc`, the comp's rows when it was
/// saved over `doc` with visibility captured.
fn recorded8(doc: &DrDocument) -> Vec<(u64, bool)> {
    rows8(doc).iter().map(|r| (r.id, r.visible)).collect()
}

/// Asserts the report and the apply for comp `comp`, saved over `saved`.
fn check8(doc: &DrDocument, saved: &DrDocument, comp: usize, what: &str) -> DrDocument {
    let recorded = ids(&rows8(saved));
    assert_eq!(
        unrecorded(&report8(doc), comp),
        expected_unrecorded(&rows8(doc), &recorded),
        "{what}: the report"
    );
    let applied = apply8(doc, comp).unwrap_or_else(|| panic!("{what}: apply changes"));
    assert_rule(&rows8(doc), &rows8(&applied), &recorded8(saved), what);
    applied
}

#[test]
fn byte_new_layers_at_the_top_and_inside_a_recorded_group_are_hidden() {
    let saved = save8(&base8(), "All");
    let g = index_of(&rows8(&saved), "G");
    let doc = add8(&saved, g, TOP, "LN");
    let g = index_of(&rows8(&doc), "G");
    let doc = add8(&doc, g, INTO_GROUP, "LN2");
    let applied = check8(&doc, &saved, 0, "new layers");
    let rows = rows8(&applied);
    assert!(!by_name(&rows, "LN").visible && !by_name(&rows, "LN2").visible);
    assert!(by_name(&rows, "G").visible && by_name(&rows, "C").visible);
}

#[test]
fn byte_every_capture_set_hides_a_new_layer() {
    let base = base8();
    let doc = comp8(
        &base,
        json!({"action":"save","name":"Pos","visibility":false,"position":true,"appearance":false}),
    )
    .unwrap();
    let doc = comp8(
        &doc,
        json!({"action":"save","name":"App","visibility":false,"position":false,"appearance":true}),
    )
    .unwrap();
    let saved = doc.clone();
    let doc = add8(&doc, 0, TOP, "LN");
    for comp in [0, 1] {
        let applied = apply8(&doc, comp).expect("the new layer is hidden");
        let rows = rows8(&applied);
        assert!(!by_name(&rows, "LN").visible, "comp {comp}");
        for row in rows8(&saved) {
            let now = rows.iter().find(|r| r.id == row.id).unwrap();
            assert_eq!(now.visible, row.visible, "comp {comp}: {}", row.name);
        }
        assert_eq!(
            unrecorded(&report8(&doc), comp),
            expected_unrecorded(&rows8(&doc), &ids(&rows8(&saved))),
        );
    }
}

#[test]
fn byte_a_pasted_group_stays_visible_and_its_children_are_hidden() {
    let source = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 255])))
        .adding_image_layer(0, square(40), "PG_a")
        .and_then(|d| d.adding_image_layer(1, square(50), "PG_b"))
        .expect("two layers");
    let source = group8(&source, &[1, 2], "PG");
    let pg = [index_of(&rows8(&source), "PG")];
    let clip = take8(
        unsafe { dr_doc_duplicate_to_new_document(&source, pg.as_ptr(), pg.len()) },
        "the clip",
    );
    let saved = save8(&base8(), "All");
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_paste_layers(&saved, &clip, 0, ABOVE, 0, 0, false, &mut err) };
    assert_eq!(owned(err), None, "the paste");
    let doc = take8(out, "the paste");
    let names: Vec<String> = unrecorded(&report8(&doc), 0)
        .into_iter()
        .map(|r| r.2)
        .collect();
    assert_eq!(names, ["PG_a", "PG_b"], "only the children");
    let applied = check8(&doc, &saved, 0, "pasted group");
    let rows = rows8(&applied);
    assert!(
        by_name(&rows, "PG").visible,
        "the pasted group stays visible"
    );
    assert!(!by_name(&rows, "PG_a").visible && !by_name(&rows, "PG_b").visible);
}

#[test]
fn byte_a_new_group_stays_visible_its_new_child_is_hidden_and_an_empty_one_too() {
    let saved = save8(&base8(), "All");
    let doc = add8(&saved, 0, TOP, "LNc");
    let at = index_of(&rows8(&doc), "LNc");
    let doc = group8(&doc, &[at], "NGb");
    let applied = check8(&doc, &saved, 0, "new group");
    let rows = rows8(&applied);
    assert!(by_name(&rows, "NGb").visible && !by_name(&rows, "LNc").visible);

    // The child removed, the empty group is no unrecorded LAYER either.
    let at = index_of(&rows8(&doc), "LNc");
    let empty = take8(unsafe { dr_doc_removing_layer(&doc, at) }, "the removal");
    assert!(unrecorded(&report8(&empty), 0).is_empty());
    let hidden = visible8(&empty, "Background", false);
    let applied = check8(&hidden, &saved, 0, "empty group");
    assert!(by_name(&rows8(&applied), "NGb").visible);
}

#[test]
fn byte_a_group_made_over_recorded_layers_stays_visible_and_they_take_their_rows() {
    let base = visible8(&base8(), "A", false);
    let saved = save8(&base, "All");
    let (a, b) = (index_of(&rows8(&saved), "A"), index_of(&rows8(&saved), "B"));
    let doc = group8(&saved, &[a, b], "NG");
    let doc = visible8(&visible8(&doc, "A", true), "B", false);
    let applied = check8(&doc, &saved, 0, "group existing");
    let rows = rows8(&applied);
    assert!(by_name(&rows, "NG").visible);
    assert!(!by_name(&rows, "A").visible && by_name(&rows, "B").visible);
}

#[test]
fn byte_a_duplicate_is_hidden_under_both_comps_and_a_duplicated_group_is_not() {
    let doc = save8(&base8(), "Shown");
    let doc = save8(&visible8(&doc, "A", false), "Hidden");
    let saved = visible8(&doc, "A", true);
    let a = index_of(&rows8(&saved), "A");
    let doc = take8(
        unsafe { dr_doc_duplicating_layer(&saved, a) },
        "duplicate A",
    );
    let g = index_of(&rows8(&doc), "G");
    let doc = take8(unsafe { dr_doc_duplicating_layer(&doc, g) }, "duplicate G");
    let fresh: Vec<Row> = rows8(&doc)
        .into_iter()
        .filter(|r| !ids(&rows8(&saved)).contains(&r.id))
        .collect();
    assert_eq!(fresh.len(), 3, "A's copy, G's copy and its child's copy");
    let shown = check8(&doc, &saved, 0, "Shown");
    let hidden_saved = visible8(&saved, "A", false);
    let hidden = apply8(&doc, 1).expect("apply Hidden");
    assert_rule(
        &rows8(&doc),
        &rows8(&hidden),
        &recorded8(&hidden_saved),
        "Hidden",
    );
    for applied in [&shown, &hidden] {
        let rows = rows8(applied);
        for copy in &fresh {
            let now = rows.iter().find(|r| r.id == copy.id).unwrap();
            assert_eq!(now.visible, copy.group, "{}", copy.name);
        }
    }
}

#[test]
fn byte_update_with_recapture_records_the_layer_and_without_it_does_not() {
    let saved = save8(&base8(), "All");
    let doc = add8(&saved, 0, TOP, "LN");

    let kept = comp8(
        &doc,
        json!({"action":"update","comp":0,"recapture":false,"comment":"later"}),
    )
    .expect("the comment changes");
    assert_eq!(unrecorded(&report8(&kept), 0).len(), 1);
    let applied = apply8(&kept, 0).expect("LN is hidden");
    assert!(!by_name(&rows8(&applied), "LN").visible);

    let updated = comp8(&doc, json!({"action":"update","comp":0})).expect("a recapture");
    assert!(unrecorded(&report8(&updated), 0).is_empty());
    let hidden = visible8(&updated, "Background", false);
    let applied = apply8(&hidden, 0).expect("the Background comes back");
    assert!(by_name(&rows8(&applied), "LN").visible, "LN keeps its row");
}

#[test]
fn byte_every_lock_still_hides_an_unrecorded_layer() {
    let saved = save8(&base8(), "All");
    let doc = add8(&saved, 0, TOP, "LN");
    let at = index_of(&rows8(&doc), "LN");
    let doc = take8(
        unsafe { dr_doc_with_layer_locks(&doc, at, LOCK_ALL) },
        "lock",
    );
    assert_eq!(doc.layers[at].locks, LOCK_ALL);
    let applied = check8(&doc, &saved, 0, "locked");
    assert!(!by_name(&rows8(&applied), "LN").visible);
}

#[test]
fn byte_an_apply_whose_unrecorded_layers_are_already_hidden_is_the_exact_no_op() {
    let saved = save8(&base8(), "All");
    let doc = visible8(&add8(&saved, 0, TOP, "LN"), "LN", false);
    assert_eq!(unrecorded(&report8(&doc), 0).len(), 1, "still unrecorded");
    assert!(apply8(&doc, 0).is_none(), "no phantom undo step");
}

#[test]
fn byte_the_rule_survives_a_save_and_a_reopen() {
    let dir = TempDir::new().unwrap();
    let saved = save8(&base8(), "All");
    let doc = add8(&saved, 0, TOP, "LN");
    let path = CString::new(dir.path().join("comp.rz").to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_doc_save_native(&doc, path.as_ptr(), std::ptr::null(), &mut err) });
    assert_eq!(owned(err), None);
    let mut err = ptr::null_mut();
    let reopened = take8(
        unsafe { dr_doc_open(path.as_ptr(), std::ptr::null(), &mut err) },
        "reopen",
    );
    assert_eq!(owned(err), None);
    assert_eq!(rows8(&reopened), rows8(&doc), "the ids persist");
    let applied = check8(&reopened, &saved, 0, "reopened");
    assert!(!by_name(&rows8(&applied), "LN").visible);
}

// ================================================================ 16 bits ==

fn rows16(doc: &PrecisionDocument) -> Vec<Row> {
    (0..doc.layer_count())
        .map(|i| {
            let l = doc.layer(i).expect("a record");
            Row {
                id: l.id(),
                name: l.name().to_string(),
                group: l.children().is_some(),
                visible: l.visible(),
            }
        })
        .collect()
}

fn take16(out: *mut PrecisionDocument, what: &str) -> PrecisionDocument {
    assert!(!out.is_null(), "{what}");
    *unsafe { Box::from_raw(out) }
}

/// One typed request; `None` is the core's exact no-op.
fn edit16(doc: &PrecisionDocument, request: Value) -> Option<PrecisionDocument> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None, "{request}");
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn must16(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    edit16(doc, request.clone()).unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn comp16(doc: &PrecisionDocument, request: Value) -> Option<PrecisionDocument> {
    let mut request = request;
    request["op"] = json!("layer_comp");
    edit16(doc, request)
}

fn save16(doc: &PrecisionDocument, name: &str) -> PrecisionDocument {
    comp16(doc, json!({"action":"save","name":name})).expect("a save")
}

fn apply16(doc: &PrecisionDocument, comp: usize) -> Option<PrecisionDocument> {
    comp16(doc, json!({"action":"apply","comp":comp}))
}

fn report16(doc: &PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let json = owned(unsafe { dr_precision_doc_layer_comps_json(doc, &mut err) });
    assert_eq!(owned(err), None);
    serde_json::from_str(&json.expect("a report")).expect("report JSON")
}

fn add16(doc: &PrecisionDocument, anchor: usize, position: &str, name: &str) -> PrecisionDocument {
    must16(
        doc,
        json!({"op":"add_layer","name":name,"anchor":anchor,"position":position}),
    )
}

fn group16(doc: &PrecisionDocument, idx: &[usize], name: &str) -> PrecisionDocument {
    must16(doc, json!({"op":"group_layers","layers":idx,"name":name}))
}

fn visible16(doc: &PrecisionDocument, name: &str, visible: bool) -> PrecisionDocument {
    let i = index_of(&rows16(doc), name);
    must16(
        doc,
        json!({"op":"layer_visible","layer":i,"visible":visible}),
    )
}

fn raster16(name: &str, v: u16, w: u32) -> Layer {
    let px = vec![[v, v, v, 65535]; (w * w) as usize];
    let image = PrecisionImage::from_rgba16(w, w, px, IccProfile::srgb(), None).unwrap();
    Layer::raster(name, image, (0, 0), 1.0, true).unwrap()
}

fn doc16(layers: Vec<Layer>) -> PrecisionDocument {
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

/// Background, A, B, and the group G holding C, in postorder.
fn base16() -> PrecisionDocument {
    let doc = doc16(vec![
        raster16("Background", 32896, W),
        raster16("A", 3000, 4),
        raster16("B", 6000, 4),
        raster16("C", 9000, 4),
    ]);
    group16(&doc, &[3], "G")
}

fn recorded16(doc: &PrecisionDocument) -> Vec<(u64, bool)> {
    rows16(doc).iter().map(|r| (r.id, r.visible)).collect()
}

fn check16(
    doc: &PrecisionDocument,
    saved: &PrecisionDocument,
    comp: usize,
    what: &str,
) -> PrecisionDocument {
    let recorded = ids(&rows16(saved));
    assert_eq!(
        unrecorded(&report16(doc), comp),
        expected_unrecorded(&rows16(doc), &recorded),
        "{what}: the report"
    );
    let applied = apply16(doc, comp).unwrap_or_else(|| panic!("{what}: apply changes"));
    assert_rule(&rows16(doc), &rows16(&applied), &recorded16(saved), what);
    applied
}

#[test]
fn sixteen_bit_new_layers_at_the_top_and_inside_a_recorded_group_are_hidden() {
    let saved = save16(&base16(), "All");
    let g = index_of(&rows16(&saved), "G");
    let doc = add16(&saved, g, "top", "LN");
    let g = index_of(&rows16(&doc), "G");
    let doc = add16(&doc, g, "into_group", "LN2");
    let applied = check16(&doc, &saved, 0, "new layers");
    let rows = rows16(&applied);
    assert!(!by_name(&rows, "LN").visible && !by_name(&rows, "LN2").visible);
    assert!(by_name(&rows, "G").visible && by_name(&rows, "C").visible);
}

#[test]
fn sixteen_bit_every_capture_set_hides_a_new_layer() {
    let doc = comp16(
        &base16(),
        json!({"action":"save","name":"Pos","visibility":false,"position":true,"appearance":false}),
    )
    .unwrap();
    let doc = comp16(
        &doc,
        json!({"action":"save","name":"App","visibility":false,"position":false,"appearance":true}),
    )
    .unwrap();
    let saved = doc.clone();
    let doc = add16(&doc, 0, "top", "LN");
    for comp in [0, 1] {
        let applied = apply16(&doc, comp).expect("the new layer is hidden");
        let rows = rows16(&applied);
        assert!(!by_name(&rows, "LN").visible, "comp {comp}");
        for row in rows16(&saved) {
            let now = rows.iter().find(|r| r.id == row.id).unwrap();
            assert_eq!(now.visible, row.visible, "comp {comp}: {}", row.name);
        }
        assert_eq!(
            unrecorded(&report16(&doc), comp),
            expected_unrecorded(&rows16(&doc), &ids(&rows16(&saved))),
        );
    }
}

#[test]
fn sixteen_bit_a_pasted_group_stays_visible_and_its_children_are_hidden() {
    let source = doc16(vec![
        raster16("Background", 0, W),
        raster16("PG_a", 20000, 4),
        raster16("PG_b", 40000, 4),
    ]);
    let source = group16(&source, &[1, 2], "PG");
    let pg = index_of(&rows16(&source), "PG");
    let clip = must16(
        &source,
        json!({"op":"duplicate_to_new_document","layers":[pg]}),
    );
    let saved = save16(&base16(), "All");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_paste_layers(
            &saved,
            &clip,
            0,
            ABOVE,
            0,
            0,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None, "the paste");
    let doc = take16(out, "the paste");
    let names: Vec<String> = unrecorded(&report16(&doc), 0)
        .into_iter()
        .map(|r| r.2)
        .collect();
    assert_eq!(names, ["PG_a", "PG_b"], "only the children");
    let applied = check16(&doc, &saved, 0, "pasted group");
    let rows = rows16(&applied);
    assert!(
        by_name(&rows, "PG").visible,
        "the pasted group stays visible"
    );
    assert!(!by_name(&rows, "PG_a").visible && !by_name(&rows, "PG_b").visible);
}

#[test]
fn sixteen_bit_a_new_group_stays_visible_and_its_new_child_is_hidden() {
    let saved = save16(&base16(), "All");
    let doc = add16(&saved, 0, "top", "LNc");
    let at = index_of(&rows16(&doc), "LNc");
    let doc = group16(&doc, &[at], "NGb");
    let applied = check16(&doc, &saved, 0, "new group");
    let rows = rows16(&applied);
    assert!(by_name(&rows, "NGb").visible && !by_name(&rows, "LNc").visible);
}

#[test]
fn sixteen_bit_a_group_made_over_recorded_layers_stays_visible_and_they_take_their_rows() {
    let saved = save16(&visible16(&base16(), "A", false), "All");
    let (a, b) = (
        index_of(&rows16(&saved), "A"),
        index_of(&rows16(&saved), "B"),
    );
    let doc = group16(&saved, &[a, b], "NG");
    let doc = visible16(&visible16(&doc, "A", true), "B", false);
    let applied = check16(&doc, &saved, 0, "group existing");
    let rows = rows16(&applied);
    assert!(by_name(&rows, "NG").visible);
    assert!(!by_name(&rows, "A").visible && by_name(&rows, "B").visible);
}

#[test]
fn sixteen_bit_a_duplicate_is_hidden_under_both_comps_and_a_duplicated_group_is_not() {
    let doc = save16(&base16(), "Shown");
    let doc = save16(&visible16(&doc, "A", false), "Hidden");
    let saved = visible16(&doc, "A", true);
    let duplicate = |doc: &PrecisionDocument, name: &str| {
        let i = index_of(&rows16(doc), name);
        let mut err = ptr::null_mut();
        let out = unsafe { dr_precision_doc_duplicate_layer(doc, i, &mut err) };
        assert_eq!(owned(err), None, "duplicate {name}");
        take16(out, name)
    };
    let doc = duplicate(&saved, "A");
    let doc = duplicate(&doc, "G");
    let fresh: Vec<Row> = rows16(&doc)
        .into_iter()
        .filter(|r| !ids(&rows16(&saved)).contains(&r.id))
        .collect();
    assert_eq!(fresh.len(), 3, "A's copy, G's copy and its child's copy");
    let shown = check16(&doc, &saved, 0, "Shown");
    let hidden_saved = visible16(&saved, "A", false);
    let hidden = apply16(&doc, 1).expect("apply Hidden");
    assert_rule(
        &rows16(&doc),
        &rows16(&hidden),
        &recorded16(&hidden_saved),
        "Hidden",
    );
    for applied in [&shown, &hidden] {
        let rows = rows16(applied);
        for copy in &fresh {
            let now = rows.iter().find(|r| r.id == copy.id).unwrap();
            assert_eq!(now.visible, copy.group, "{}", copy.name);
        }
    }
}

#[test]
fn sixteen_bit_update_with_recapture_records_the_layer_and_without_it_does_not() {
    let saved = save16(&base16(), "All");
    let doc = add16(&saved, 0, "top", "LN");

    let kept = comp16(
        &doc,
        json!({"action":"update","comp":0,"recapture":false,"comment":"later"}),
    )
    .expect("the comment changes");
    assert_eq!(unrecorded(&report16(&kept), 0).len(), 1);
    let applied = apply16(&kept, 0).expect("LN is hidden");
    assert!(!by_name(&rows16(&applied), "LN").visible);

    let updated = comp16(&doc, json!({"action":"update","comp":0})).expect("a recapture");
    assert!(unrecorded(&report16(&updated), 0).is_empty());
    let hidden = visible16(&updated, "Background", false);
    let applied = apply16(&hidden, 0).expect("the Background comes back");
    assert!(by_name(&rows16(&applied), "LN").visible, "LN keeps its row");
}

#[test]
fn sixteen_bit_every_lock_still_hides_an_unrecorded_layer() {
    let saved = save16(&base16(), "All");
    let doc = add16(&saved, 0, "top", "LN");
    let at = index_of(&rows16(&doc), "LN");
    let doc = must16(&doc, json!({"op":"layer_lock","layer":at,"locks":LOCK_ALL}));
    let applied = check16(&doc, &saved, 0, "locked");
    assert!(!by_name(&rows16(&applied), "LN").visible);
}

#[test]
fn sixteen_bit_an_apply_whose_unrecorded_layers_are_already_hidden_is_the_exact_no_op() {
    let saved = save16(&base16(), "All");
    let doc = visible16(&add16(&saved, 0, "top", "LN"), "LN", false);
    assert_eq!(unrecorded(&report16(&doc), 0).len(), 1, "still unrecorded");
    assert!(apply16(&doc, 0).is_none(), "no phantom undo step");
}

#[test]
fn sixteen_bit_the_rule_survives_a_save_and_a_reopen() {
    let dir = TempDir::new().unwrap();
    let saved = save16(&base16(), "All");
    let doc = add16(&saved, 0, "top", "LN");
    let path = CString::new(dir.path().join("comp.rz").to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_save_native(&doc, path.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    });
    assert_eq!(owned(err), None);
    let mut err = ptr::null_mut();
    let reopened = take16(
        unsafe { dr_precision_doc_open(path.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "reopen",
    );
    assert_eq!(owned(err), None);
    assert_eq!(rows16(&reopened), rows16(&doc), "the ids persist");
    let applied = check16(&reopened, &saved, 0, "reopened");
    assert!(!by_name(&rows16(&applied), "LN").visible);
}
