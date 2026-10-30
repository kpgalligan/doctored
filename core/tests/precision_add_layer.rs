//! Phase 12E-2b: the TYPED `add_layer` structure op — `precision_insert`'s
//! `add_layer_at` through `{"op":"add_layer"}` and `dr_precision_doc_edit`, at
//! u16 AND at 32-bit float.
//!
//! The oracles are written here and never read off the implementation: the
//! expected structure table after each of the three positions, the SAMPLES of
//! the new record (every component zero, canvas extent, offset (0,0)), the
//! INPUT document's own projection as the oracle for the result's (a fully
//! transparent Normal leaf at opacity 1 contributes nothing, at any of the
//! three slots), and the input's table and ids for purity.
//!
//! Tolerances, fixed BEFORE anything was measured: every render comparison in
//! this file is EXACT — bit for bit at u16 and at float alike — because the
//! claim is that the projection did not change at all, not that it stayed
//! close.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

const ABOVE: InsertPosition = InsertPosition::Above;
const TOP: InsertPosition = InsertPosition::Top;
const INTO: InsertPosition = InsertPosition::IntoGroup;

// ------------------------------------------------------------- plumbing --

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn spelling(position: InsertPosition) -> &'static str {
    match position {
        ABOVE => "above",
        TOP => "top",
        INTO => "into_group",
    }
}

fn add(name: &str, anchor: usize, position: InsertPosition) -> Value {
    json!({"op":"add_layer","name":name,"anchor":anchor,"position":spelling(position)})
}

/// One request through the public typed edit surface, answering either the
/// new document or the refusal the core phrased.
fn edit(doc: &PrecisionDocument, request: Value) -> Result<PrecisionDocument, String> {
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
    if out.is_null() {
        return Err(owned(err).unwrap_or_else(|| "no error and no document".into()));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

fn added(
    doc: &PrecisionDocument,
    name: &str,
    anchor: usize,
    position: InsertPosition,
) -> PrecisionDocument {
    edit(doc, add(name, anchor, position))
        .unwrap_or_else(|e| panic!("{name} at {position:?} from {anchor}: {e}"))
}

/// The refusal the core phrased; panics when the request succeeded, since a
/// document is not a message.
fn refusal(doc: &PrecisionDocument, request: Value) -> String {
    match edit(doc, request.clone()) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

// --------------------------------------------- the independent structure --

#[derive(Debug, PartialEq, Eq, Clone)]
struct Row {
    name: String,
    level: usize,
    clipped: bool,
}

/// The document's records in POSTORDER, walked here rather than read off any
/// core report.
fn table(doc: &PrecisionDocument) -> Vec<Row> {
    fn walk(layers: &[Layer], level: usize, out: &mut Vec<Row>) {
        for layer in layers {
            if let Some(children) = layer.children() {
                walk(children, level + 1, out);
            }
            out.push(Row {
                name: layer.name().to_string(),
                level,
                clipped: layer.clipped(),
            });
        }
    }
    let mut out = Vec::new();
    walk(doc.root_layers(), 0, &mut out);
    out
}

fn expect(doc: &PrecisionDocument, want: &[(&str, usize, bool)]) {
    let want: Vec<Row> = want
        .iter()
        .map(|(name, level, clipped)| Row {
            name: (*name).to_string(),
            level: *level,
            clipped: *clipped,
        })
        .collect();
    assert_eq!(table(doc), want);
}

fn ids(doc: &PrecisionDocument) -> Vec<u64> {
    (0..doc.layer_count())
        .map(|i| doc.layer_id(i).expect("an id"))
        .collect()
}

fn index_of(doc: &PrecisionDocument, name: &str) -> usize {
    table(doc)
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn render_float(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()
}

/// What this file requires of a NEW record, asserted off the record's own
/// fields: canvas extent, offset (0,0), Normal at opacity 1, visible,
/// unmasked, and every stored component exactly zero.
fn assert_transparent_canvas_leaf(doc: &PrecisionDocument, name: &str) {
    let index = index_of(doc, name);
    let record = doc.layer(index).expect("the new record");
    assert_eq!(record.opacity(), 1.0, "{name} is at opacity 1");
    assert!(record.visible(), "{name} is visible");
    assert!(record.mask().is_none(), "{name} carries no mask");
    assert!(record.children().is_none(), "{name} is a leaf");
    assert_eq!(record.offset(), (0, 0), "{name} sits at the origin");
    let image = doc.layer_source(index).expect("a raster record");
    assert_eq!(
        image.dimensions(),
        doc.dimensions(),
        "{name} covers the canvas"
    );
    match (image.to_rgba16().ok(), image.to_rgba32().ok()) {
        (Some(samples), _) => assert!(
            samples.iter().all(|p| *p == [0u16; 4]),
            "{name} is transparent at u16"
        ),
        (_, Some(samples)) => assert!(
            samples.iter().all(|p| *p == [0.0f32; 4]),
            "{name} is transparent at float"
        ),
        _ => panic!("{name} has samples"),
    }
}

// -------------------------------------------------------------- fixtures --

fn image(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None).unwrap()
}

fn raster(name: &str, value: u16) -> Layer {
    let px: Vec<[u16; 4]> = (0..4u16)
        .map(|i| [value + i * 11, value / 2 + i * 7, value / 3 + i * 3, 65535])
        .collect();
    Layer::raster(name, image(px), (0, 0), 1.0, true).unwrap()
}

fn mask(values: Vec<u16>) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(values.len() as u32, 1, values, None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap()
}

/// `[Base, ClipBase, ClipTop(clipped), Group{Inner}]` — a clip run and an
/// isolated group, which is every shape the slot rule has something to say
/// about here.
fn fixture() -> PrecisionDocument {
    let doc = PrecisionDocument::from_parts(
        4,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster("Base", 9000),
            raster("ClipBase", 21000).with_mask(Some(mask(vec![0, 30000, 50000, 65535]))),
            raster("ClipTop", 33000).with_clipped(true),
            Layer::isolated_normal("Group", vec![raster("Inner", 41000)], 1.0, true).unwrap(),
        ],
    )
    .unwrap();
    expect(
        &doc,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("Group", 0, false),
        ],
    );
    doc
}

fn float_fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    let base = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).ok()?;
    assert!(base.is_float());
    Some(base)
}

// ------------------------------------------------ (a) the three positions --

#[test]
fn a_the_three_positions_land_where_the_rule_says() {
    let doc = fixture();

    let above = added(&doc, "New", 1, ABOVE);
    expect(
        &above,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            // The next sibling at level 0 is `ClipTop`, which is clipped, so
            // the new record JOINS the run rather than re-basing it.
            ("New", 0, true),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("Group", 0, false),
        ],
    );

    let into = added(&doc, "New", 4, INTO);
    expect(
        &into,
        &[
            ("Base", 0, false),
            ("ClipBase", 0, false),
            ("ClipTop", 0, true),
            ("Inner", 1, false),
            ("New", 1, false),
            ("Group", 0, false),
        ],
    );

    // TOP is the top of the ROOT level, whatever the anchor names.
    for anchor in 0..doc.layer_count() {
        let top = added(&doc, "New", anchor, TOP);
        expect(
            &top,
            &[
                ("Base", 0, false),
                ("ClipBase", 0, false),
                ("ClipTop", 0, true),
                ("Inner", 1, false),
                ("Group", 0, false),
                ("New", 0, false),
            ],
        );
    }
}

#[test]
fn b_the_new_record_is_a_transparent_canvas_leaf_and_mints_once() {
    let doc = fixture();
    let before = table(&doc);
    let before_ids = ids(&doc);
    for (anchor, position) in [(1, ABOVE), (4, INTO), (0, TOP)] {
        let out = added(&doc, "New", anchor, position);
        assert_transparent_canvas_leaf(&out, "New");
        // Every prior id carried, in order, and exactly ONE fresh mint.
        let fresh = out.layer_id(index_of(&out, "New")).expect("an id");
        let carried: Vec<u64> = ids(&out).into_iter().filter(|&id| id != fresh).collect();
        assert_eq!(carried, before_ids, "{position:?} carries every prior id");
        assert!(fresh != 0 && !before_ids.contains(&fresh), "a fresh id");
        // PURITY: the document that was asked is the document it was.
        assert_eq!(table(&doc), before);
        assert_eq!(ids(&doc), before_ids);
    }
}

// ------------------------------------------------------- (c) the renders --

#[test]
fn c_the_projection_is_bit_identical_at_u16() {
    // A fully transparent Normal leaf at opacity 1 contributes nothing at any
    // slot — including inside the isolated group and inside the clip run,
    // where a WRONGLY unclipped insert would have become the run's new base
    // and re-based `ClipTop` onto its (empty) alpha.
    let doc = fixture();
    let want = render(&doc);
    for (anchor, position) in [(1, ABOVE), (0, ABOVE), (4, INTO), (0, TOP)] {
        let out = added(&doc, "New", anchor, position);
        assert_eq!(
            render(&out),
            want,
            "{position:?} from {anchor} leaves the projection exactly"
        );
    }
}

#[test]
fn d_float_admits_the_three_positions_bit_identically() {
    let Some(base) = float_fixture() else {
        eprintln!("skipping: {FLOAT_FIXTURE} is absent");
        return;
    };
    // A clip run and a group at float, built out of the fixture's one raster.
    let second = base.duplicate_layer(0).unwrap().unwrap();
    let grouped = second
        .group_layers(&[1], "Group", false)
        .expect("grouping")
        .expect("a change")
        .0;
    let third = grouped.duplicate_layer(0).unwrap().unwrap();
    let clipped = edit(
        &third,
        json!({"op":"layer_clipped","layer":1,"clipped":true}),
    )
    .expect("a clip flag");
    assert_eq!(clipped.float_refusal(), None, "the float fixture renders");
    let want = render_float(&clipped);
    let group = index_of(&clipped, "Group");
    for (anchor, position) in [(0, ABOVE), (group, INTO), (0, TOP)] {
        let out = added(&clipped, "New", anchor, position);
        assert_transparent_canvas_leaf(&out, "New");
        assert_eq!(
            render_float(&out),
            want,
            "{position:?} from {anchor} is exact at float"
        );
    }
    // The family is ADMITTED at float by name, and its target is the document.
    assert_eq!(
        capability("layer_add", "document", 32)["supported"],
        json!(true)
    );
    assert_eq!(
        capability("layer_add", "document", 16)["supported"],
        json!(true)
    );
    assert_eq!(
        capability("layer_add", "raster", 16)["supported"],
        json!(false)
    );
}

// ----------------------------------------------------------- (e) refusals --

#[test]
fn e_every_refusal_is_named() {
    let doc = fixture();
    assert_eq!(
        refusal(&doc, add("New", 99, ABOVE)),
        "high-depth layer index is out of range"
    );
    assert_eq!(
        refusal(&doc, add("New", 0, INTO)),
        "into_group requires a group layer"
    );
    // An unknown position spelling is refused by the request parser rather
    // than defaulted to `above`.
    assert!(refusal(
        &doc,
        json!({"op":"add_layer","name":"New","anchor":0,"position":"sideways"})
    )
    .starts_with("invalid high-depth layer property"));
    // Neither field has a default: the op names its anchor AND its slot, so a
    // caller can never get a placement it did not ask for.
    assert!(
        refusal(&doc, json!({"op":"add_layer","name":"New","anchor":0}))
            .starts_with("invalid high-depth layer property")
    );
    assert!(refusal(
        &doc,
        json!({"op":"add_layer","name":"New","position":"above"})
    )
    .starts_with("invalid high-depth layer property"));
    // And an unknown field refuses rather than being ignored.
    assert!(refusal(
        &doc,
        json!({"op":"add_layer","name":"New","anchor":0,"position":"above","extra":1})
    )
    .starts_with("invalid high-depth layer property"));
}
