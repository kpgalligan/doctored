//! Phase 12E-2: POSITIONAL insertion at BYTE depth — `layer_insert`'s one
//! slot rule, `DrDocument::insert_slot` / `adding_layer_at` and the
//! `dr_doc_adding_layer_at` export.
//!
//! The oracle is written out here and never read off the implementation: the
//! expected structure table (name, level, clipped) after each of the three
//! positions, `Arc::ptr_eq` on every surviving record's pixels and mask (an
//! insertion moves nothing and copies no sample), the input document's own
//! table afterwards (purity), and an f64 composite for the renders — which is
//! EXACT at 8 bits here, because every fixture layer is opaque, Normal and at
//! opacity 1, so the only arithmetic the projection performs is the invert
//! kernel's own `255 - c`.
//!
//! Tolerances, fixed before anything was measured: 8-bit renders are compared
//! EXACTLY (no tolerance at all).

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, LayerRaster, MaskKind};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_layer_count};
use doctored_core::ffi_insert::dr_doc_adding_layer_at;
use doctored_core::layer_insert::InsertPosition;
use image::RgbaImage;

use crate::common;
use common::{adjust_meta, opaque_pattern, solid};

const ABOVE: InsertPosition = InsertPosition::Above;
const TOP: InsertPosition = InsertPosition::Top;
const INTO: InsertPosition = InsertPosition::IntoGroup;

// ------------------------------------------------- the independent table --

/// One entry as this file describes it: name, NESTING LEVEL and the clipping
/// flag — walked off the stack's own fields, never off a core report.
#[derive(Debug, PartialEq, Eq, Clone)]
struct Row {
    name: String,
    level: u16,
    clipped: bool,
}

fn table(doc: &DrDocument) -> Vec<Row> {
    doc.layers
        .iter()
        .map(|l| Row {
            name: l.name.clone(),
            level: l.depth,
            clipped: l.clipped,
        })
        .collect()
}

fn expect(doc: &DrDocument, want: &[(&str, u16, bool)]) {
    let got = table(doc);
    let want: Vec<Row> = want
        .iter()
        .map(|(name, level, clipped)| Row {
            name: (*name).to_string(),
            level: *level,
            clipped: *clipped,
        })
        .collect();
    assert_eq!(got, want);
}

fn ids(doc: &DrDocument) -> Vec<u64> {
    doc.layers.iter().map(|l| l.id).collect()
}

fn index_of(doc: &DrDocument, name: &str) -> usize {
    doc.layers
        .iter()
        .position(|l| l.name == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

/// Every record the insert did not create, by NAME, with its pixel buffer and
/// mask plane compared by ADDRESS: an insertion splices one node in and must
/// share every other allocation.
fn assert_allocations_shared(before: &DrDocument, after: &DrDocument) {
    for layer in &before.layers {
        let mirror = after
            .layers
            .iter()
            .find(|l| l.name == layer.name)
            .unwrap_or_else(|| panic!("{} survived", layer.name));
        assert!(
            LayerRaster::ptr_eq(&layer.pixels, &mirror.pixels),
            "{}'s pixels are the same allocation",
            layer.name
        );
        match (&layer.mask, &mirror.mask) {
            (Some(a), Some(b)) => assert!(
                Arc::ptr_eq(a, b),
                "{}'s mask is the same allocation",
                layer.name
            ),
            (None, None) => {}
            _ => panic!("{}'s mask presence changed", layer.name),
        }
        assert_eq!(layer.id, mirror.id, "{}'s identity is carried", layer.name);
    }
}

// -------------------------------------------------------------- fixtures --

/// `[Base, ClipBase, ClipTop(clipped), Group{Inner}, Styled]` — a clip run, a
/// group and a styled entry, which is every shape the slot rule has something
/// to say about. `ClipBase` carries a mask so the allocation check has one.
fn fixture() -> DrDocument {
    let doc = DrDocument::from_pixels(opaque_pattern(4, 2));
    let doc = doc.with_layer_name(0, "Base").expect("rename");
    let doc = doc
        .adding_image_layer(0, solid(4, 2, [10, 20, 30, 255]), "ClipBase")
        .expect("ClipBase");
    let doc = doc.add_mask(1, MaskKind::RevealAll).expect("a mask");
    let doc = doc
        .adding_image_layer(1, solid(4, 2, [40, 50, 60, 255]), "ClipTop")
        .expect("ClipTop");
    let doc = doc.with_layer_clipped(2, true).expect("clip");
    let doc = doc
        .adding_image_layer(2, solid(4, 2, [70, 80, 90, 255]), "Inner")
        .expect("Inner");
    let (doc, group, _, _) = doc.group_layers(&[3], "Group").expect("Group");
    assert_eq!(group, 4);
    let doc = doc
        .adding_image_layer(4, solid(4, 2, [11, 12, 13, 255]), "Styled")
        .expect("Styled");
    let style: Arc<doctored_core::style::LayerStyle> = Arc::new(common::parse(
        r#"{"effects":[{"type":"drop_shadow","use_global_light":false,"angle":90.0,"distance":2.0,"size":1.0,"opacity":0.5}]}"#,
    ));
    let doc = doc.set_layer_style(5, Some(style)).expect("a style");
    expect(
        &doc,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Styled", 0, false),
        ],
    );
    doc
}

/// The invert adjustment this file's renders are measured against: a
/// transparent canvas-sized layer whose meta makes the compositor treat it as
/// one (`adjust.rs`) — the byte form of a live adjustment.
fn as_invert(doc: &DrDocument, idx: usize) -> DrDocument {
    let mut out = doc.clone();
    out.layers[idx].meta = Some(adjust_meta("invert", "{}"));
    out
}

fn flat(doc: &DrDocument) -> RgbaImage {
    doc.flattened().expect("a projection")
}

// ------------------------------------------------- (a) the three positions --

#[test]
fn a_the_three_positions_land_where_the_rule_says() {
    let doc = fixture();

    // ABOVE the clip BASE: immediately above its own record, at its level,
    // and CLIPPED — the next sibling at level 0 is `ClipTop`, which is
    // clipped, so the new entry joins the run instead of re-basing it.
    let slot = doc.insert_slot(1, ABOVE).expect("a slot");
    assert_eq!((slot.at, slot.level, slot.clipped), (2, 0, true));
    let above = doc.adding_layer_at(1, ABOVE, "New").expect("inserted");
    expect(
        &above,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("New", 0, true),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Styled", 0, false),
        ],
    );

    // ABOVE a GROUP is above the group, never inside it.
    let slot = doc.insert_slot(4, ABOVE).expect("a slot");
    assert_eq!((slot.at, slot.level, slot.clipped), (5, 0, false));

    // INTO the group: its TOP CHILD, one level deeper, unclipped (there is no
    // sibling above it inside the group).
    let slot = doc.insert_slot(4, INTO).expect("a slot");
    assert_eq!((slot.at, slot.level, slot.clipped), (4, 1, false));
    let into = doc.adding_layer_at(4, INTO, "New").expect("inserted");
    expect(
        &into,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("New", 1, false),
            ("Group", 0, false),
            ("Styled", 0, false),
        ],
    );

    // TOP: the top of the ROOT level, whatever the anchor names.
    for anchor in 0..doc.layers.len() {
        let slot = doc.insert_slot(anchor, TOP).expect("a slot");
        assert_eq!((slot.at, slot.level, slot.clipped), (6, 0, false));
    }
    let top = doc.adding_layer_at(3, TOP, "New").expect("inserted");
    expect(
        &top,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Styled", 0, false),
            ("New", 0, false),
        ],
    );

    // `Above` on the top root and `Top` agree, and both agree with the
    // untouched `adding_layer` — the placement every existing caller has.
    let legacy = doc.adding_layer(5, "New").expect("inserted");
    assert_eq!(table(&legacy), table(&top));
    assert_eq!(
        table(&doc.adding_layer_at(5, ABOVE, "New").expect("inserted")),
        table(&top)
    );
}

#[test]
fn b_identities_allocations_and_the_input_are_untouched() {
    let doc = fixture();
    let before = table(&doc);
    let before_ids = ids(&doc);
    for (anchor, position) in [(1, ABOVE), (4, INTO), (0, TOP)] {
        let out = doc.adding_layer_at(anchor, position, "New").expect("ok");
        assert_allocations_shared(&doc, &out);
        // Exactly ONE minted id, and it is not one the document already had.
        let fresh = out.layers[index_of(&out, "New")].id;
        assert!(fresh != 0 && !before_ids.contains(&fresh), "a fresh id");
        assert_eq!(
            out.layers.iter().filter(|l| l.id == fresh).count(),
            1,
            "the mint is unique"
        );
        // PURITY: the document that was asked is the document it was.
        assert_eq!(table(&doc), before);
        assert_eq!(ids(&doc), before_ids);
    }
}

// ------------------------------------------------------- (c) the renders --

#[test]
fn c_an_inserted_adjustment_matches_an_f64_oracle() {
    // A one-layer opaque document; the oracle is `255 - c` per RGB channel
    // with alpha carried, computed here and compared EXACTLY.
    let base = opaque_pattern(4, 2);
    let doc = DrDocument::from_pixels(base.clone());
    let inserted = doc.adding_layer_at(0, ABOVE, "Invert").expect("inserted");
    let inverted = as_invert(&inserted, 1);
    let got = flat(&inverted);
    for (x, y, px) in base.enumerate_pixels() {
        let want = [255u8 - px.0[0], 255 - px.0[1], 255 - px.0[2], px.0[3]];
        assert_eq!(got.get_pixel(x, y).0, want, "invert at ({x},{y})");
    }

    // The same adjustment INSIDE a group: the group is isolated and Normal at
    // opacity 1 over an opaque backdrop, so its render is the same invert.
    let two = doc
        .adding_image_layer(0, solid(4, 2, [90, 40, 200, 255]), "Inner")
        .expect("Inner");
    let (grouped, group, _, _) = two.group_layers(&[1], "Group").expect("Group");
    let into = grouped
        .adding_layer_at(group, INTO, "Invert")
        .expect("inserted");
    let into = as_invert(&into, index_of(&into, "Invert"));
    let got = flat(&into);
    for px in got.pixels() {
        assert_eq!(px.0, [165, 215, 55, 255], "invert inside the group");
    }
}

#[test]
fn d_an_opacity_zero_insertion_is_bit_identical() {
    // The strongest "the neighbours were not disturbed" claim available: an
    // adjustment that WOULD change every pixel, inserted at each of the three
    // positions at opacity 0, must leave the projection byte for byte.
    let doc = fixture();
    let want = flat(&doc);
    for (anchor, position) in [(1, ABOVE), (4, INTO), (0, TOP)] {
        let out = doc.adding_layer_at(anchor, position, "Invert").expect("ok");
        let idx = index_of(&out, "Invert");
        let out = as_invert(&out, idx);
        let out = out.with_layer_opacity(idx, 0.0).expect("opacity 0");
        assert_eq!(
            flat(&out).into_raw(),
            want.clone().into_raw(),
            "{position:?} at opacity 0 changes nothing"
        );
    }
}

#[test]
fn e_an_insert_above_a_clip_base_joins_the_run() {
    // [Base, ClipBase (opaque on the left half, absent on the right),
    //  ClipTop (clipped, fully transparent)] — so the only visible effect of
    // the inserted invert is where the clip run confines it.
    let base = RgbaImage::from_fn(4, 1, |x, _| image::Rgba([20 + x as u8, 60, 200, 255]));
    let clip_base = RgbaImage::from_fn(4, 1, |x, _| {
        if x < 2 {
            image::Rgba([100, 150, 30, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    });
    let doc = DrDocument::from_pixels(base.clone());
    let doc = doc
        .adding_image_layer(0, clip_base.clone(), "ClipBase")
        .expect("ClipBase");
    let doc = doc
        .adding_image_layer(1, RgbaImage::new(4, 1), "ClipTop")
        .expect("ClipTop");
    let doc = doc.with_layer_clipped(2, true).expect("clip");

    let out = doc.adding_layer_at(1, ABOVE, "Invert").expect("inserted");
    assert!(out.layers[2].clipped, "the new entry joined the run");
    assert!(out.layers[3].clipped, "ClipTop's own flag is untouched");
    assert!(!out.layers[1].clipped, "the base is still the base");
    let out = as_invert(&out, 2);
    let got = flat(&out);

    // The oracle: inside the base's footprint the backdrop is ClipBase's own
    // colour, inverted; outside it the clip unit contributes nothing and the
    // projection is `Base`. An UNCLIPPED insert would have inverted the right
    // half too, which is exactly the difference this asserts.
    for x in 0..4u32 {
        let want = if x < 2 {
            [155, 105, 225, 255]
        } else {
            [20 + x as u8, 60, 200, 255]
        };
        assert_eq!(got.get_pixel(x, 0).0, want, "clip-run join at x={x}");
    }
}

// ----------------------------------------------------------- (f) refusals --

#[test]
fn f_every_refusal_is_named() {
    let doc = fixture();
    assert_eq!(
        doc.insert_slot(99, ABOVE).unwrap_err(),
        "layer insert anchor is out of range"
    );
    assert_eq!(
        doc.insert_slot(99, TOP).unwrap_err(),
        "layer insert anchor is out of range"
    );
    assert_eq!(
        doc.insert_slot(0, INTO).unwrap_err(),
        "into_group requires a group layer"
    );
    assert!(doc.adding_layer_at(99, ABOVE, "New").is_none());
    assert!(doc.adding_layer_at(0, INTO, "New").is_none());
    assert_eq!(
        InsertPosition::from_code(3).unwrap_err(),
        "unsupported layer insert position"
    );
    assert_eq!(InsertPosition::from_code(0), Ok(InsertPosition::Above));
    assert_eq!(InsertPosition::from_code(1), Ok(InsertPosition::Top));
    assert_eq!(InsertPosition::from_code(2), Ok(InsertPosition::IntoGroup));

    // The nesting ceiling. A group WITH CHILDREN cannot sit at the cap —
    // its children would be one level past it — so the deepest such group a
    // document can hold is one level ABOVE the cap and `into_group` on it
    // lands exactly AT the cap, which is admitted…
    let mut deep = DrDocument::from_pixels(opaque_pattern(2, 1));
    let mut inner = 0usize;
    for _ in 0..doctored_core::doc_group::MAX_GROUP_DEPTH {
        let (next, group, _, _) = deep.group_layers(&[inner], "G").expect("nested");
        deep = next;
        inner = group;
    }
    let level = deep.layers[0].depth;
    assert_eq!(level, doctored_core::doc_group::MAX_GROUP_DEPTH);
    let innermost = deep
        .layers
        .iter()
        .position(|l| l.depth == level - 1)
        .expect("the innermost group");
    let slot = deep.insert_slot(innermost, INTO).expect("at the cap");
    assert_eq!(slot.level, usize::from(level));

    // …but an EMPTIED group can sit AT the cap (`doc_group::validate_structure`
    // admits a group with no children at any admitted level), and
    // `into_group` on THAT one lands a level too deep. Built here so the
    // sentence is asserted rather than argued: a root layer, grouped, its
    // child deleted, and the empty container moved into the deepest group.
    let spare = deep.layers.len() - 1;
    let deep = deep
        .adding_layer(spare, "Spare")
        .expect("a spare root layer");
    let (deep, container, _, _) = deep
        .group_layers(&[spare + 1], "Empty")
        .expect("the container");
    let deep = deep.removing_layer(container - 1).expect("the child goes");
    let empty = index_of(&deep, "Empty");
    let deep = deep
        .move_layer_to(empty, 1, level)
        .expect("the empty group lands inside the deepest group");
    let empty = index_of(&deep, "Empty");
    assert_eq!(deep.layers[empty].depth, level, "it sits AT the cap");
    assert!(deep.layers[empty].kind == doctored_core::doc::LayerKind::Group);
    assert_eq!(
        deep.insert_slot(empty, INTO).unwrap_err(),
        "layer insert nesting exceeds its limit"
    );
    assert!(deep.adding_layer_at(empty, INTO, "New").is_none());
    // ABOVE the same entry is still admitted, at the cap's own level: only
    // the level PAST the ceiling refuses.
    assert_eq!(
        deep.insert_slot(empty, ABOVE).expect("above is fine").level,
        usize::from(level)
    );

    // The TYPED ceiling shares this very branch (`layer_insert::slot`) but is
    // unreachable by construction, which is why `precision_insert.rs` asserts
    // no typed twin of the sentence: the typed limit is a subtree HEIGHT
    // checked in `PrecisionDocumentLayer::group`, so a chain deep enough to
    // put a group AT `MAX_GROUP_DEPTH` is refused when it is BUILT — a typed
    // group can only reach level `MAX_GROUP_DEPTH - 1`, where `into_group`
    // lands exactly at the cap.
}

// ---------------------------------------------------------------- (g) FFI --

#[test]
fn g_the_export_tolerates_null_and_refuses_a_bad_code() {
    let name = CString::new("New").unwrap();
    // NULL document, NULL name, and an unknown position code.
    assert!(unsafe { dr_doc_adding_layer_at(ptr::null(), 0, 0, name.as_ptr()) }.is_null());
    let doc = Box::into_raw(Box::new(fixture()));
    assert!(unsafe { dr_doc_adding_layer_at(doc, 0, 0, ptr::null()) }.is_null());
    assert!(unsafe { dr_doc_adding_layer_at(doc, 0, 7, name.as_ptr()) }.is_null());
    assert!(unsafe { dr_doc_adding_layer_at(doc, 99, 0, name.as_ptr()) }.is_null());
    // …and the three codes that ARE the enum.
    for (code, count) in [(0u32, 7usize), (1, 7), (2, 7)] {
        let anchor = if code == 2 { 4 } else { 0 };
        let out = unsafe { dr_doc_adding_layer_at(doc, anchor, code, name.as_ptr()) };
        assert!(!out.is_null(), "code {code} inserts");
        assert_eq!(unsafe { dr_doc_layer_count(out) }, count);
        unsafe { dr_doc_free(out) };
    }
    unsafe { dr_doc_free(doc) };
}
