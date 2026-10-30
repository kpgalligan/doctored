//! Phase 12E-1: the TYPED twins of the four pure structure operations —
//! `move_layer_to`, `arrange_layer`, subset `group_layers` / `ungroup_layer`
//! and the set `remove_layers` — driven through the public
//! `dr_precision_doc_edit` request surface at u16 AND at 32-bit float.
//!
//! The oracle is the BYTE contract, written out here as the exact expected
//! structure table (name, nesting level, parent, id) after each op rather
//! than read off the typed implementation. Alongside it: every surviving
//! record's pixels and mask plane are POINTER-equal afterwards (a structural
//! op moves nodes and touches no sample), the channel list is untouched, an
//! adjustment layer and a DESCRIBED (text) layer travel through, ids follow
//! `doc_identity`'s rule (a move/delete/ungroup carries, a container mints),
//! every refusal is named, and a linear-light document is unchanged in its
//! refusal walk.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_image_storage_identity;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

// ------------------------------------------------------------- plumbing --

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn edit(doc: &PrecisionDocument, request: Value) -> Result<Option<PrecisionDocument>, String> {
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
    if let Some(message) = owned(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn changed(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    edit(doc, request.clone())
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} reported no change"))
}

fn refusal(doc: &PrecisionDocument, request: Value) -> String {
    match edit(doc, request.clone()) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

fn plan(doc: &PrecisionDocument, request: Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if let Some(message) = owned(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok(serde_json::from_str(&owned(out).unwrap()).unwrap())
}

// --------------------------------------------- the independent structure --

/// One record as this file describes it: name, NESTING LEVEL (0 at the top),
/// the index of the enclosing group (`None` at the top level) and the id.
#[derive(Debug, PartialEq, Eq, Clone)]
struct Row {
    name: String,
    depth: usize,
    parent: Option<usize>,
    id: u64,
}

/// The document's records in postorder, walked here rather than read from
/// any core report: children first, then their group, exactly the numbering
/// `PrecisionDocument::layer` indexes.
fn table(doc: &PrecisionDocument) -> Vec<Row> {
    fn walk(
        layers: &[Layer],
        depth: usize,
        parent: Option<usize>,
        next: &mut usize,
        out: &mut Vec<Row>,
    ) {
        for layer in layers {
            let children = layer.children();
            // The group's own index is known only after its subtree is
            // numbered, so its children's `parent` is patched in afterwards.
            let first = out.len();
            if let Some(children) = children {
                walk(children, depth + 1, None, next, out);
            }
            let index = *next;
            *next += 1;
            for row in &mut out[first..] {
                if row.depth == depth + 1 && row.parent.is_none() {
                    row.parent = Some(index);
                }
            }
            out.push(Row {
                name: layer.name().to_string(),
                depth,
                parent,
                id: layer.id(),
            });
        }
    }
    let mut out = Vec::new();
    walk(doc.root_layers(), 0, None, &mut 0, &mut out);
    out
}

fn names(doc: &PrecisionDocument) -> Vec<String> {
    table(doc).into_iter().map(|row| row.name).collect()
}

fn ids(doc: &PrecisionDocument) -> Vec<u64> {
    table(doc).into_iter().map(|row| row.id).collect()
}

fn row(doc: &PrecisionDocument, name: &str) -> Row {
    table(doc)
        .into_iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

/// The raster's STORAGE IDENTITY, so "the pixels were not touched" is an
/// identity claim and not a comparison of equal values.
fn pixel_address(doc: &PrecisionDocument, name: &str) -> usize {
    let index = table(doc).iter().position(|r| r.name == name).expect(name);
    let source = doc.layer_source(index).expect("a raster record");
    unsafe { dr_precision_image_storage_identity(source) as usize }
}

fn mask_address(doc: &PrecisionDocument, name: &str) -> usize {
    let index = table(doc).iter().position(|r| r.name == name).expect(name);
    doc.layer(index)
        .and_then(|l| l.mask())
        .expect("a masked record")
        .plane()
        .samples()
        .as_ptr() as usize
}

// -------------------------------------------------------------- fixtures --

fn image(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None).unwrap()
}

fn raster(name: &str) -> Layer {
    let px: Vec<[u16; 4]> = (0..4u16)
        .map(|i| [1001 + i, 2003 + i, 3007 + i, 65535])
        .collect();
    Layer::raster(name, image(px), (0, 0), 1.0, true).unwrap()
}

fn adjustment(name: &str) -> Layer {
    let description =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    Layer::adjustment(name, &description, 1.0, true, &IccProfile::srgb()).unwrap()
}

fn mask(values: Vec<u16>) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(values.len() as u32, 1, values, None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap()
}

fn channel(name: &str) -> PrecisionChannel {
    PrecisionChannel::new(
        name,
        PrecisionPlane::from_u16(4, 1, vec![0, 100, 200, 65535], None).unwrap(),
    )
    .unwrap()
}

/// `[A, B, Group{C, D}, E]` bottom-first, with a mask on `B`, an adjustment
/// as `D` and one saved channel — a stack every one of the five ops has
/// something to say about.
fn fixture() -> PrecisionDocument {
    let layers = vec![
        raster("A"),
        raster("B").with_mask(Some(mask(vec![0, 123, 32769, 65535]))),
        Layer::pass_through("G", vec![raster("C"), adjustment("D")], 1.0, true).unwrap(),
        raster("E"),
    ];
    PrecisionDocument::from_parts_and_channels(
        4,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
        vec![channel("Alpha 1")],
    )
    .unwrap()
}

fn float_fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    let base = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).ok()?;
    assert!(base.is_float());
    // The fixture opens as one `Background` raster; a second root and a group
    // give the structure ops something to move.
    let second = base.duplicate_layer(0).unwrap().unwrap();
    let renamed = changed(
        &second,
        json!({"op":"rename_layer","layer":1,"name":"Second"}),
    );
    // The request carries no `pass_through`, so it asks for the serde default
    // — Pass Through — which a float document honours.
    let grouped = changed(
        &renamed,
        json!({"op":"group_layers","layers":[1],"name":"G"}),
    );
    assert!(
        grouped.layer(2).unwrap().is_pass_through(),
        "a float container is minted Pass Through when asked"
    );
    assert_eq!(
        grouped.float_refusal(),
        None,
        "the float renderer accepts the grouped document"
    );
    let third = grouped.duplicate_layer(0).unwrap().unwrap();
    Some(changed(
        &third,
        json!({"op":"rename_layer","layer":1,"name":"Third"}),
    ))
}

// ------------------------------------------------ (a) flatten and rebuild --

#[test]
fn a_move_out_and_back_restores_the_document_exactly() {
    // The flatten/rebuild round trip, asserted end to end: a move and its
    // inverse must return a document whose structure, ids, pixel buffers and
    // mask planes are the ones it started with — which is only true if
    // flattening and rebuilding move nodes and copy nothing.
    let doc = fixture();
    let before = table(&doc);
    let addresses: Vec<usize> = ["A", "B", "C", "E"]
        .iter()
        .map(|name| pixel_address(&doc, name))
        .collect();
    let mask_before = mask_address(&doc, "B");

    // "E" is the top root at postorder index 5 (the group closes its own
    // subtree at 4); move it to the bottom, then back above everything.
    let down = changed(
        &doc,
        json!({"op":"move_layer_to","from":5,"to":0,"depth":0}),
    );
    assert_eq!(names(&down), ["E", "A", "B", "C", "D", "G"]);
    let back = changed(
        &down,
        json!({"op":"move_layer_to","from":0,"to":5,"depth":0}),
    );

    assert_eq!(table(&back), before, "the structure returns exactly");
    for (name, address) in ["A", "B", "C", "E"].iter().zip(&addresses) {
        assert_eq!(
            pixel_address(&back, name),
            *address,
            "{name} kept its pixels"
        );
    }
    assert_eq!(mask_address(&back, "B"), mask_before);
    assert_eq!(back.channels().len(), 1, "channels are untouched");
    assert_eq!(back.channels()[0].name(), "Alpha 1");
    assert_eq!(
        back.adjustment_description(3),
        doc.adjustment_description(3),
        "the adjustment travelled"
    );
}

// ---------------------------------------------------------- (b) the move --

#[test]
fn move_layer_to_reparents_and_reports_the_exact_table() {
    let doc = fixture();
    let before = table(&doc);
    // "A" (index 0) into the group. `to` numbers the list with A's subtree
    // already drained — [B, C, D, G, E] — so slot 1 is C's place and A lands
    // BELOW C, at the bottom of the group.
    let inside = changed(
        &doc,
        json!({"op":"move_layer_to","from":0,"to":1,"depth":1}),
    );
    assert_eq!(names(&inside), ["B", "A", "C", "D", "G", "E"]);
    for name in ["A", "C", "D"] {
        let entry = row(&inside, name);
        assert_eq!(entry.depth, 1, "{name} is inside the group");
        assert_eq!(entry.parent, Some(4), "{name} sits under G at index 4");
    }
    assert_eq!(row(&inside, "B").depth, 0);
    assert_eq!(row(&inside, "E").depth, 0);
    // Slot 2 puts it ABOVE C instead, which is the same op with a different
    // destination and not a different rule.
    let higher = changed(
        &doc,
        json!({"op":"move_layer_to","from":0,"to":2,"depth":1}),
    );
    assert_eq!(names(&higher), ["B", "C", "A", "D", "G", "E"]);

    // Every id carried: a move renumbers, it never mints.
    let mut sorted_before: Vec<u64> = before.iter().map(|r| r.id).collect();
    let mut sorted_after = ids(&inside);
    sorted_before.sort_unstable();
    sorted_after.sort_unstable();
    assert_eq!(sorted_after, sorted_before, "a move carries every id");
    assert_eq!(pixel_address(&inside, "A"), pixel_address(&doc, "A"));
    assert_eq!(mask_address(&inside, "B"), mask_address(&doc, "B"));

    // The exact no-op: back where it came from at its own level.
    assert!(
        edit(
            &doc,
            json!({"op":"move_layer_to","from":0,"to":0,"depth":0})
        )
        .unwrap()
        .is_none(),
        "an identical result is NULL, never a copy"
    );
    // Refusals, each by name.
    assert!(refusal(
        &doc,
        json!({"op":"move_layer_to","from":9,"to":0,"depth":0})
    )
    .contains("out of range"));
    assert!(
        refusal(
            &doc,
            json!({"op":"move_layer_to","from":0,"to":0,"depth":99})
        )
        .contains("nesting"),
        "a depth past the cap refuses by name"
    );
    // A leaf at level 0 that would close a non-empty level-1 run is a
    // malformed tree, not a silent copy.
    assert!(
        refusal(
            &doc,
            json!({"op":"move_layer_to","from":0,"to":3,"depth":0})
        )
        .contains("malformed"),
        "a destination that breaks the depth sequence refuses by name"
    );
    // And the topmost record has to sit at depth 0.
    assert!(refusal(
        &doc,
        json!({"op":"move_layer_to","from":5,"to":5,"depth":1})
    )
    .contains("malformed"));
}

// ------------------------------------------------------- (c) the arrange --

#[test]
fn arrange_layer_moves_within_the_level_only() {
    let doc = fixture();
    // "A" forward among its siblings: A, B, G, E at the top level.
    let forward = changed(
        &doc,
        json!({"op":"arrange_layer","layer":0,"how":"forward"}),
    );
    assert_eq!(names(&forward), ["B", "A", "C", "D", "G", "E"]);
    assert_eq!(row(&forward, "A").depth, 0);
    let front = changed(&doc, json!({"op":"arrange_layer","layer":0,"how":"front"}));
    assert_eq!(names(&front), ["B", "C", "D", "G", "E", "A"]);
    // A group carries its whole subtree.
    let group_back = changed(&doc, json!({"op":"arrange_layer","layer":4,"how":"back"}));
    assert_eq!(names(&group_back), ["C", "D", "G", "A", "B", "E"]);
    // Inside the group the level is the group's children alone.
    let inner = changed(&doc, json!({"op":"arrange_layer","layer":2,"how":"front"}));
    assert_eq!(names(&inner), ["A", "B", "D", "C", "G", "E"]);
    assert_eq!(
        row(&inner, "C").depth,
        1,
        "a sibling reorder never leaves the level"
    );

    // No-ops: already there.
    for (layer, how) in [
        (0usize, "backward"),
        (0, "back"),
        (5, "front"),
        (5, "forward"),
    ] {
        assert!(
            edit(&doc, json!({"op":"arrange_layer","layer":layer,"how":how}))
                .unwrap()
                .is_none(),
            "{layer}/{how} is already where it would land"
        );
    }
    assert!(
        refusal(&doc, json!({"op":"arrange_layer","layer":9,"how":"front"}))
            .contains("out of range")
    );
    assert!(refusal(
        &doc,
        json!({"op":"arrange_layer","layer":0,"how":"sideways"})
    )
    .contains("invalid high-depth layer property"));
    let mut sorted = ids(&front);
    sorted.sort_unstable();
    let mut expected = ids(&doc);
    expected.sort_unstable();
    assert_eq!(sorted, expected, "a reorder carries every id");
}

// --------------------------------------------------- (d) subset grouping --

#[test]
fn group_layers_gathers_a_subset_and_reports_what_it_changed() {
    let doc = fixture();
    // A NON-CONTIGUOUS subset of the top level: A (0) and G (4). The
    // subtrees gather into the TOPMOST given entry's slot, so B — which sat
    // between them — ends up below the new group.
    let request = json!({"op":"group_layers","layers":[0,4],"name":"Set"});
    let report = plan(&doc, request.clone()).unwrap()["report"].clone();
    let grouped = changed(&doc, request);
    assert_eq!(names(&grouped), ["B", "A", "C", "D", "G", "Set", "E"]);
    let set = row(&grouped, "Set");
    assert_eq!(set.depth, 0);
    assert_eq!(report["group"], json!(5));
    assert_eq!(
        table(&grouped)
            .iter()
            .position(|r| r.name == "Set")
            .unwrap(),
        5
    );
    for name in ["A", "G"] {
        assert_eq!(row(&grouped, name).depth, 1, "{name} is inside Set");
        assert_eq!(row(&grouped, name).parent, Some(5));
    }
    assert_eq!(
        row(&grouped, "C").depth,
        2,
        "the nested subtree went with it"
    );
    assert_eq!(report["cleared_clip"], json!([]));
    // B is the surviving entry whose relative order changed.
    assert_eq!(report["reordered"], json!([0]));
    assert_eq!(row(&grouped, "B").depth, 0);

    // The container MINTS; every member keeps its own id.
    let before: Vec<u64> = ids(&doc);
    for name in ["A", "B", "C", "D", "G", "E"] {
        assert!(
            before.contains(&row(&grouped, name).id),
            "{name} kept its id"
        );
    }
    assert!(!before.contains(&set.id), "the container minted");
    assert_eq!(pixel_address(&grouped, "A"), pixel_address(&doc, "A"));
    assert_eq!(mask_address(&grouped, "B"), mask_address(&doc, "B"));
    assert_eq!(grouped.channels().len(), 1);

    // The bottom-most member loses a clipping mask it can no longer honour.
    let clipped = changed(&doc, json!({"op":"layer_clipped","layer":1,"clipped":true}));
    let request = json!({"op":"group_layers","layers":[1,4],"name":"Clip"});
    let report = plan(&clipped, request.clone()).unwrap()["report"].clone();
    let out = changed(&clipped, request);
    // The gathered block starts at slot 1 (A is kept below it), so the
    // bottom-most member lands at index 1.
    assert_eq!(names(&out), ["A", "B", "C", "D", "G", "Clip", "E"]);
    assert_eq!(report["cleared_clip"], json!([1]));
    assert!(!out.layer(1).unwrap().clipped(), "the flag was released");

    // Refusals, each by name.
    assert!(
        refusal(&doc, json!({"op":"group_layers","layers":[0,2],"name":"X"}))
            .contains("one shared parent")
    );
    assert!(
        refusal(&doc, json!({"op":"group_layers","layers":[],"name":"X"}))
            .contains("at least one layer")
    );
    assert!(
        refusal(&doc, json!({"op":"group_layers","layers":[9],"name":"X"}))
            .contains("out of range")
    );
}

// -------------------------------------------------------- (e) ungrouping --

#[test]
fn ungroup_layer_frees_the_children_and_reports_where_they_landed() {
    let doc = fixture();
    let request = json!({"op":"ungroup_layer","layer":4});
    let report = plan(&doc, request.clone()).unwrap()["report"].clone();
    let out = changed(&doc, request);
    assert_eq!(names(&out), ["A", "B", "C", "D", "E"]);
    for name in ["C", "D"] {
        assert_eq!(row(&out, name).depth, 0, "{name} took the group's level");
        assert_eq!(row(&out, name).parent, None);
    }
    assert_eq!(report["landed"], json!([2, 3]));
    assert_eq!(report["cleared_clip"], json!([]));
    // Every id carried; the group's own is gone with it.
    let before = ids(&doc);
    for name in ["A", "B", "C", "D", "E"] {
        assert!(before.contains(&row(&out, name).id), "{name} kept its id");
    }
    assert_eq!(out.layer_count(), 5);
    assert_eq!(pixel_address(&out, "C"), pixel_address(&doc, "C"));
    assert_eq!(mask_address(&out, "B"), mask_address(&doc, "B"));
    assert_eq!(
        out.adjustment_description(3),
        doc.adjustment_description(3),
        "the adjustment travelled"
    );

    // The baseless-clip rule: C was at the bottom of its level inside the
    // group, so a clipped flag on it had nothing to clip to; out at the
    // parent level it would suddenly clip to B, so it is RELEASED and named.
    let clipped = changed(&doc, json!({"op":"layer_clipped","layer":2,"clipped":true}));
    let request = json!({"op":"ungroup_layer","layer":4});
    let report = plan(&clipped, request.clone()).unwrap()["report"].clone();
    let out = changed(&clipped, request);
    assert_eq!(report["cleared_clip"], json!([2]));
    assert!(!out.layer(2).unwrap().clipped());

    assert!(
        refusal(&doc, json!({"op":"ungroup_layer","layer":0})).contains("requires a group record")
    );
    assert!(refusal(&doc, json!({"op":"ungroup_layer","layer":9})).contains("out of range"));
}

// ------------------------------------------------------- (f) the deletion --

#[test]
fn remove_layers_drops_subsumed_entries_and_refuses_an_empty_document() {
    let doc = fixture();
    let out = changed(&doc, json!({"op":"remove_layers","layers":[1,4]}));
    assert_eq!(names(&out), ["A", "E"], "a group takes its whole subtree");
    let before = ids(&doc);
    for name in ["A", "E"] {
        assert!(before.contains(&row(&out, name).id), "{name} kept its id");
    }
    assert_eq!(pixel_address(&out, "A"), pixel_address(&doc, "A"));
    assert_eq!(out.channels().len(), 1, "channels are untouched");

    // A set holding a group AND one of its own children takes the subtree
    // once — the byte `independent_roots` rule.
    let subsumed = changed(&doc, json!({"op":"remove_layers","layers":[2,4]}));
    assert_eq!(names(&subsumed), ["A", "B", "E"]);

    assert!(
        refusal(&doc, json!({"op":"remove_layers","layers":[0,1,4,5]}))
            .contains("at least one record"),
        "deleting every record refuses by name"
    );
    assert!(refusal(&doc, json!({"op":"remove_layers","layers":[]})).contains("at least one layer"));
    assert!(refusal(&doc, json!({"op":"remove_layers","layers":[9]})).contains("out of range"));
}

// ------------------------------------------ (g) a described layer travels --

#[test]
fn a_described_layer_survives_every_structure_op() {
    use doctored_core::ffi_artwork::dr_precision_doc_artwork_metadata;
    let doc = fixture();
    let description = json!({"type":"text","version":1,"payload":"hello"}).to_string();
    let value = CString::new(description.clone()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_artwork_metadata(&doc, 0, value.as_ptr(), LIMIT, 0, &mut err) };
    let message = owned(err);
    assert!(
        message.is_none() && !out.is_null(),
        "the description must be accepted: {message:?}"
    );
    let described = *unsafe { Box::from_raw(out) };
    assert_eq!(described.artwork_description(0), Some(&description[..]));
    let pointer = described.artwork_description(0).unwrap().as_ptr();

    for request in [
        json!({"op":"move_layer_to","from":0,"to":4,"depth":0}),
        json!({"op":"arrange_layer","layer":0,"how":"front"}),
        json!({"op":"group_layers","layers":[0,1],"name":"S"}),
        json!({"op":"remove_layers","layers":[5]}),
    ] {
        let out = changed(&described, request.clone());
        let index = table(&out)
            .iter()
            .position(|r| r.name == "A")
            .unwrap_or_else(|| panic!("{request}: A survives"));
        assert_eq!(
            out.artwork_description(index).map(|d| d.as_ptr()),
            Some(pointer),
            "{request} shared the description's storage"
        );
    }
}

// ------------------------------------------------------------ (h) float --

#[test]
fn the_five_ops_answer_on_a_float_document_too() {
    let Some(doc) = float_fixture() else { return };
    assert!(doc.is_float());
    assert_eq!(names(&doc), ["Background", "Third", "Second", "G"]);
    let before = ids(&doc);
    let background = pixel_address(&doc, "Background");

    let moved = changed(
        &doc,
        json!({"op":"move_layer_to","from":1,"to":2,"depth":1}),
    );
    assert_eq!(names(&moved), ["Background", "Second", "Third", "G"]);
    assert_eq!(row(&moved, "Third").depth, 1);
    assert_eq!(pixel_address(&moved, "Background"), background);

    let arranged = changed(&doc, json!({"op":"arrange_layer","layer":0,"how":"front"}));
    assert_eq!(names(&arranged), ["Third", "Second", "G", "Background"]);

    // Pass Through is asked for EXPLICITLY here, and a float document mints
    // it as every other depth does.
    let grouped = changed(
        &doc,
        json!({"op":"group_layers","layers":[0,3],"name":"Set","pass_through":true}),
    );
    assert_eq!(
        names(&grouped),
        ["Third", "Background", "Second", "G", "Set"]
    );
    assert_eq!(row(&grouped, "Set").depth, 0);
    assert_eq!(row(&grouped, "Background").depth, 1);
    assert!(
        grouped.layer(4).unwrap().is_pass_through(),
        "the container is Pass Through at float"
    );
    assert_eq!(
        grouped.float_refusal(),
        None,
        "the float renderer accepts the result"
    );

    let ungrouped = changed(&doc, json!({"op":"ungroup_layer","layer":3}));
    assert_eq!(names(&ungrouped), ["Background", "Third", "Second"]);
    assert_eq!(row(&ungrouped, "Second").depth, 0);

    let removed = changed(&doc, json!({"op":"remove_layers","layers":[3]}));
    assert_eq!(names(&removed), ["Background", "Third"]);
    assert_eq!(pixel_address(&removed, "Background"), background);
    for name in ["Background", "Third"] {
        assert!(
            before.contains(&row(&removed, name).id),
            "{name} kept its id"
        );
    }
    assert!(removed.is_float(), "the mode survives a structure edit");
}

// ------------------------------------------------ (i) the refusal walk --

#[test]
fn a_linear_light_document_still_refuses_exactly_what_it_did() {
    let doc = fixture()
        .with_evaluation(doctored_core::doc_evaluation::Evaluation::LinearLight)
        .unwrap()
        .unwrap();
    // A pure structure edit moves records and evaluates nothing, so the
    // linear-light walk neither admits nor refuses anything new: the ops
    // answer, and the policy is carried.
    let out = changed(&doc, json!({"op":"remove_layers","layers":[4]}));
    assert_eq!(names(&out), ["A", "B", "E"]);
    assert_eq!(
        out.evaluation(),
        doctored_core::doc_evaluation::Evaluation::LinearLight,
        "a rebuild carries the policy"
    );
    assert!(out.render_plan().is_ok(), "the refusal walk is unchanged");
}
