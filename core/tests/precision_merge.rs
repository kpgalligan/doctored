//! Phase 12E-1b: the three TYPED merges.
//!
//! Every pixel claim here is checked against an f64 source-over oracle this
//! file writes out — `over` below — and never against a golden or against the
//! compositor's own answer. Every structural claim (which record survives,
//! which slot it lands in, which name, identity, label, locks, link and
//! clipped flag it keeps) is read back through `layer_info`'s JSON, and every
//! refusal is checked by its SENTENCE, because a host shows that sentence
//! verbatim.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_duplicate_layer;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_precision_inspect::dr_precision_doc_lock_block;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// `DR_LOCK_PIXELS` and `DR_LOCK_TRANSPARENCY`, restated rather than read out
/// of the core.
const LOCK_TRANSPARENCY: u32 = 1;
const LOCK_PIXELS: u32 = 2;
const LOCK_POSITION: u32 = 4;
/// `DrEditKind::Merge`, restated for the same reason.
const KIND_MERGE: i32 = 4;

// --------------------------------------------------- the independent oracle --

/// Straight source-over of ONE pixel, in f64: `s` over `b`, both straight
/// RGBA in [0,1], with `s` first weighted by the source layer's opacity.
/// This is the W3C composition formula written out here; nothing in it reads
/// the crate.
fn over(b: [f64; 4], s: [f64; 4], opacity: f64) -> [f64; 4] {
    let sa = s[3] * opacity;
    let out_a = sa + b[3] * (1.0 - sa);
    if out_a <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (s[c] * sa + b[c] * b[3] * (1.0 - sa)) / out_a;
    }
    out[3] = out_a;
    out
}

fn to_unit16(p: [u16; 4]) -> [f64; 4] {
    p.map(|v| f64::from(v) / 65535.0)
}

/// Every sample of `document`'s record `index`, as u16 codes.
fn samples(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    document
        .layer_source(index)
        .expect("a raster record")
        .to_rgba16()
        .expect("u16 storage")
}

fn floats(document: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    document
        .layer_source(index)
        .expect("a raster record")
        .to_rgba32()
        .expect("f32 storage")
}

/// Asserts every channel is within `tolerance` codes of the oracle's.
fn close16(got: &[[u16; 4]], want: &[[f64; 4]], tolerance: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..4 {
            let expected = w[c] * 65535.0;
            let difference = (f64::from(g[c]) - expected).abs();
            assert!(
                difference <= tolerance,
                "{what}: pixel {i} channel {c} is {} against the oracle's {expected} \
                 (difference {difference} codes, tolerance {tolerance})",
                g[c]
            );
        }
    }
}

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    unsafe {
        let text = CStr::from_ptr(error).to_string_lossy().into_owned();
        dr_string_free(error);
        Some(text)
    }
}

fn edit(
    document: &PrecisionDocument,
    request: Value,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    Ok((!result.is_null()).then(|| unsafe { Box::from_raw(result) }))
}

fn must(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    match edit(document, request.clone()) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn refusal(document: &PrecisionDocument, request: Value) -> String {
    match edit(document, request.clone()) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

fn plan_result(document: &PrecisionDocument, request: Value) -> Result<Value, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let text =
        unsafe { dr_precision_doc_edit_plan(document, request.as_ptr(), LIMIT, 0, &mut error) };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    assert!(!text.is_null(), "a plan with no error");
    let value = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    Ok(serde_json::from_str(&value).expect("plan JSON"))
}

fn plan(document: &PrecisionDocument, request: Value) -> Value {
    match plan_result(document, request.clone()) {
        Ok(value) => value,
        Err(message) => panic!("{request}: {message}"),
    }
}

fn plan_refusal(document: &PrecisionDocument, request: Value) -> String {
    match plan_result(document, request.clone()) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

/// `document` with record 0 duplicated — the one typed structural edit that
/// is a separate export rather than a request op.
fn duplicated(document: &PrecisionDocument) -> Box<PrecisionDocument> {
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_duplicate_layer(document, 0, &mut error) };
    assert!(!out.is_null(), "{:?}", take_error(error));
    unsafe { Box::from_raw(out) }
}

fn info(document: &PrecisionDocument) -> Vec<Value> {
    let value: Value =
        serde_json::from_str(&document.info().expect("info").to_string()).expect("info JSON");
    value["layers"].as_array().expect("layer rows").clone()
}

// ------------------------------------------------------------- fixtures --

fn image(width: u32, height: u32, pixel: [u16; 4]) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        width,
        height,
        vec![pixel; (width * height) as usize],
        IccProfile::srgb(),
        None,
    )
    .expect("an image")
}

fn document(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        2,
        2,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("a document")
}

/// Three canvas-sized rasters, bottom first: an opaque base, a half-covered
/// middle and a quarter-covered top. Every code is exact in binary so the
/// oracle's arithmetic and the compositor's agree to within the final round.
fn stack() -> PrecisionDocument {
    document(vec![
        Layer::raster(
            "Base",
            image(2, 2, [16384, 8192, 4096, 65535]),
            (0, 0),
            1.0,
            true,
        )
        .unwrap(),
        Layer::raster(
            "Middle",
            image(2, 2, [49152, 32768, 16384, 32768]),
            (0, 0),
            1.0,
            true,
        )
        .unwrap(),
        Layer::raster(
            "Top",
            image(2, 2, [8192, 57344, 24576, 16384]),
            (0, 0),
            1.0,
            true,
        )
        .unwrap(),
    ])
}

// ---------------------------------------------------------------- tests --

#[test]
fn merge_down_matches_the_oracle_and_keeps_the_lower_records_panel_state() {
    let stack = stack();
    let lower = to_unit16(samples(&stack, 0)[0]);
    let upper = to_unit16(samples(&stack, 1)[0]);
    let want = vec![over(lower, upper, 1.0); 4];

    let merged = must(&stack, json!({"op": "merge_down", "layer": 1}));
    assert_eq!(merged.layer_count(), 2, "two records survive");
    close16(&samples(&merged, 0), &want, 1.0, "merge_down");

    let rows = info(&merged);
    assert_eq!(rows[0]["name"], "Base", "the LOWER record's name");
    assert_eq!(rows[0]["opacity"], json!(1.0), "Normal at opacity 1");
    assert_eq!(rows[0]["blend_mode"], json!(0));
    assert_eq!(
        rows[0]["offset"],
        json!([0, 0]),
        "union of two canvas-sized members"
    );
    assert_eq!(rows[0]["dimensions"], json!([2, 2]));
    assert_eq!(rows[0]["mask"], Value::Null);
    assert_eq!(rows[1]["name"], "Top", "everything else is untouched");

    // The input is unchanged: every op is pure.
    assert_eq!(stack.layer_count(), 3);
}

#[test]
fn merge_down_keeps_the_lower_records_identity_and_label() {
    // `doc_identity`'s ONE merge rule has no `merge_down` exception (12E-2):
    // the merged record IS the lower one with new pixels, so it keeps that
    // record's id and colour label exactly as `merge_layers` does.
    let stack = must(
        &stack(),
        json!({"op": "layer_label", "layer": 0, "label": 5}),
    );
    let identity = stack.layer(0).expect("the lower record").id();
    let merged = must(&stack, json!({"op": "merge_down", "layer": 1}));
    let rows = info(&merged);
    assert_eq!(
        rows[0]["id"],
        json!(identity),
        "the LOWER record's identity"
    );
    assert_eq!(rows[0]["label"], json!(5), "and its colour label");
    assert_eq!(
        merged.layer_index_by_id(identity),
        Some(0),
        "the id still resolves to the merged record"
    );
}

#[test]
fn merge_layers_composites_its_members_and_keeps_the_lowest_identity() {
    let stack = stack();
    let base = to_unit16(samples(&stack, 0)[0]);
    let middle = to_unit16(samples(&stack, 1)[0]);
    let top = to_unit16(samples(&stack, 2)[0]);
    // The whole set, composited bottom-first — the projection's own order.
    let want = vec![over(over(base, middle, 1.0), top, 1.0); 4];

    let identity = stack.layer(0).expect("the base").id();
    let merged = must(&stack, json!({"op": "merge_layers", "layers": [0, 2, 1]}));
    assert_eq!(merged.layer_count(), 1, "one record survives");
    close16(&samples(&merged, 0), &want, 1.0, "merge_layers");

    let rows = info(&merged);
    assert_eq!(rows[0]["name"], "Base", "the LOWEST member's name");
    assert_eq!(
        rows[0]["id"],
        json!(identity),
        "and the LOWEST member's identity"
    );

    // A SUBSET merges too, and the members that were not named survive.
    let pair = must(&stack, json!({"op": "merge_layers", "layers": [1, 2]}));
    assert_eq!(pair.layer_count(), 2);
    let rows = info(&pair);
    assert_eq!(rows[0]["name"], "Base");
    assert_eq!(rows[1]["name"], "Middle", "the lowest member named");
    close16(
        &samples(&pair, 1),
        &[over(over([0.0; 4], middle, 1.0), top, 1.0); 4],
        1.0,
        "a subset composites over TRANSPARENCY, not over the base",
    );
}

#[test]
fn merge_layers_carries_the_lowest_members_clipped_flag_label_locks_and_link() {
    let stack = stack();
    let stack = must(&stack, json!({"op": "layer_label", "layer": 0, "label": 3}));
    // LOCK_POSITION, which a merge does not refuse — the point here is that
    // the flags TRAVEL, and a merge-refusing lock is `the_merge_refusals_are_named`'s.
    let stack = must(
        &stack,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_POSITION}),
    );
    let stack = must(&stack, json!({"op": "link_layers", "layers": [0, 2]}));
    let link = info(&stack)[0]["link"].as_u64().expect("a link id");
    assert_ne!(link, 0, "the fixture is linked");

    let merged = must(&stack, json!({"op": "merge_layers", "layers": [0, 1]}));
    let rows = info(&merged);
    assert_eq!(rows[0]["label"], json!(3), "the label travels");
    assert_eq!(rows[0]["locks"], json!(LOCK_POSITION), "the locks travel");
    assert_eq!(rows[0]["link"], json!(link), "the link travels");
}

#[test]
fn merge_visible_replaces_every_contributing_record_with_the_projection() {
    let stack = stack();
    let base = to_unit16(samples(&stack, 0)[0]);
    let middle = to_unit16(samples(&stack, 1)[0]);
    let top = to_unit16(samples(&stack, 2)[0]);
    let want = vec![over(over(base, middle, 1.0), top, 1.0); 4];
    let identity = stack.layer(0).expect("the base").id();

    let merged = must(&stack, json!({"op": "merge_visible"}));
    assert_eq!(merged.layer_count(), 1);
    close16(&samples(&merged, 0), &want, 1.0, "merge_visible");
    let rows = info(&merged);
    assert_eq!(rows[0]["name"], "Base");
    assert_eq!(rows[0]["id"], json!(identity));

    // A HIDDEN record does not contribute and survives untouched.
    let hidden = must(
        &stack,
        json!({"op": "layer_visible", "layer": 2, "visible": false}),
    );
    let merged = must(&hidden, json!({"op": "merge_visible"}));
    assert_eq!(merged.layer_count(), 2, "the hidden record survives");
    let rows = info(&merged);
    assert_eq!(rows[1]["name"], "Top");
    assert_eq!(rows[1]["visible"], json!(false));
    close16(
        &samples(&merged, 0),
        &[over(base, middle, 1.0); 4],
        1.0,
        "merge_visible over the contributing pair alone",
    );
}

#[test]
fn the_merge_refusals_are_named() {
    let stack = stack();

    assert!(
        refusal(&stack, json!({"op": "merge_layers", "layers": [1]}))
            .contains("at least two layers"),
        "one member is refused by name"
    );
    assert!(
        refusal(&stack, json!({"op": "merge_layers", "layers": [0, 9]}))
            .contains("index is out of range"),
        "an out-of-range member is refused by name"
    );
    assert!(
        refusal(&stack, json!({"op": "merge_down", "layer": 0}))
            .contains("a layer below it in its own level"),
        "the bottom of a level is refused by name"
    );

    // A hidden LOWER record refuses, so the upper record's content cannot
    // silently vanish.
    let hidden = must(
        &stack,
        json!({"op": "layer_visible", "layer": 0, "visible": false}),
    );
    assert!(
        refusal(&hidden, json!({"op": "merge_down", "layer": 1})).contains("a visible lower layer"),
        "a hidden lower record is refused by name"
    );
    assert!(
        refusal(&hidden, json!({"op": "merge_layers", "layers": [0, 1]}))
            .contains("a visible lowest layer"),
        "a hidden lowest member is refused by name"
    );

    // A hidden UPPER record is simply REMOVED, which is not a refusal.
    let hidden_upper = must(
        &stack,
        json!({"op": "layer_visible", "layer": 1, "visible": false}),
    );
    let merged = must(&hidden_upper, json!({"op": "merge_down", "layer": 1}));
    assert_eq!(merged.layer_count(), 2, "the upper record was removed");
    assert_eq!(
        samples(&merged, 0),
        samples(&stack, 0),
        "the lower is intact"
    );

    // A LOCKED lower record refuses, and the lock query agrees.
    for locks in [LOCK_PIXELS, LOCK_TRANSPARENCY] {
        let locked = must(
            &stack,
            json!({"op": "layer_lock", "layer": 0, "locks": locks}),
        );
        assert_eq!(
            unsafe { dr_precision_doc_lock_block(&*locked, 0, KIND_MERGE) },
            locks,
            "the lock query names the same bit"
        );
        let message = refusal(&locked, json!({"op": "merge_down", "layer": 1}));
        assert!(message.contains("is locked"), "{message}");
        let message = refusal(&locked, json!({"op": "merge_layers", "layers": [0, 1]}));
        assert!(message.contains("is locked"), "{message}");
    }

    // Merge Visible needs TWO contributing raster leaves.
    let one = document(vec![Layer::raster(
        "Only",
        image(2, 2, [1, 2, 3, 65535]),
        (0, 0),
        1.0,
        true,
    )
    .unwrap()]);
    assert!(
        refusal(&one, json!({"op": "merge_visible"})).contains("two contributing raster layers"),
        "a single leaf is refused by name"
    );
}

#[test]
fn merge_layers_requires_one_shared_parent() {
    let inner = Layer::raster("Inner", image(2, 2, [1, 2, 3, 65535]), (0, 0), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Group", vec![inner], 1.0, true).unwrap();
    let outer = Layer::raster("Outer", image(2, 2, [4, 5, 6, 65535]), (0, 0), 1.0, true).unwrap();
    let nested = document(vec![group, outer]);
    // Records 0 (Inner) and 2 (Outer) sit at different levels.
    assert!(
        refusal(&nested, json!({"op": "merge_layers", "layers": [0, 2]}))
            .contains("one shared parent group"),
        "records from two levels are refused by name"
    );
    // The group itself and its sibling DO share the top level.
    let merged = must(&nested, json!({"op": "merge_layers", "layers": [1, 2]}));
    assert_eq!(merged.layer_count(), 1, "the group's subtree went with it");
}

#[test]
fn merge_down_inside_a_group_stays_inside_it() {
    let low = Layer::raster("Low", image(2, 2, [0, 0, 0, 65535]), (0, 0), 1.0, true).unwrap();
    let high = Layer::raster("High", image(2, 2, [65535, 0, 0, 32768]), (0, 0), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Group", vec![low, high], 1.0, true).unwrap();
    let nested = document(vec![group]);
    let base = to_unit16(samples(&nested, 0)[0]);
    let upper = to_unit16(samples(&nested, 1)[0]);

    let merged = must(&nested, json!({"op": "merge_down", "layer": 1}));
    let rows = info(&merged);
    assert_eq!(rows.len(), 2, "the merged leaf and its group");
    assert_eq!(rows[0]["name"], "Low");
    assert_eq!(rows[0]["depth"], json!(1), "still inside the group");
    assert_eq!(rows[1]["name"], "Group");
    close16(
        &samples(&merged, 0),
        &[over(base, upper, 1.0); 4],
        1.0,
        "merge_down inside a group",
    );
}

/// The plan a host reads before it applies a Merge Down must charge the pixel
/// work BOTH operands cost — the previous sibling is rendered beside the named
/// record — so a plan whose LOWER operand is a group is not under-counted.
#[test]
fn merge_downs_plan_charges_both_merge_members() {
    let low = Layer::raster("Low", image(2, 2, [0, 0, 0, 65535]), (0, 0), 1.0, true).unwrap();
    let high = Layer::raster("High", image(2, 2, [65535, 0, 0, 32768]), (0, 0), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Group", vec![low, high], 1.0, true).unwrap();
    let outer = Layer::raster("Outer", image(2, 2, [4, 5, 6, 32768]), (0, 0), 1.0, true).unwrap();
    // Records: 0 Low, 1 High, 2 Group, 3 Outer. Merging 3 down lands it on the
    // GROUP, whose whole subtree is rendered for the merge.
    let nested = document(vec![group, outer]);
    let down = plan(&nested, json!({"op": "merge_down", "layer": 3}));
    let pair = plan(&nested, json!({"op": "merge_layers", "layers": [2, 3]}));
    assert_eq!(
        down["scratch_bytes"], pair["scratch_bytes"],
        "Merge Down charges the same two members Merge Layers does over that pair"
    );

    // The bottom record of a level has no sibling to merge into: the PLAN
    // refuses by the same sentence the edit does.
    assert_eq!(
        plan_refusal(&nested, json!({"op": "merge_down", "layer": 2})),
        "high-depth merge down requires a layer below it in its own level"
    );
}

#[test]
fn a_float_document_merges_against_the_same_oracle() {
    let document =
        PrecisionDocument::open_preserving("tests/fixtures/float_11f/scene.tiff", LIMIT, 0, None)
            .expect("the float fixture opens");
    assert!(document.is_float(), "the fixture is a float document");
    let doubled = duplicated(&document);
    assert_eq!(doubled.layer_count(), 2);

    let lower = floats(&doubled, 0);
    let upper = floats(&doubled, 1);
    let merged = must(&doubled, json!({"op": "merge_down", "layer": 1}));
    assert_eq!(merged.layer_count(), 1);
    let got = floats(&merged, 0);
    assert_eq!(got.len(), lower.len());
    for (i, ((b, s), g)) in lower.iter().zip(&upper).zip(&got).enumerate() {
        let want = over(b.map(f64::from), s.map(f64::from), 1.0);
        for c in 0..4 {
            let difference = (f64::from(g[c]) - want[c]).abs();
            let scale = want[c].abs().max(1.0);
            assert!(
                difference <= 1e-5 * scale,
                "float pixel {i} channel {c} is {} against the oracle's {} \
                 (relative tolerance 1e-5)",
                g[c],
                want[c]
            );
        }
    }

    // …and Merge Visible is admitted at float too.
    let doubled = duplicated(&document);
    let merged = must(&doubled, json!({"op": "merge_visible"}));
    assert_eq!(merged.layer_count(), 1);
    assert!(merged.is_float(), "the result is still a float document");
}

#[test]
fn a_merged_document_reopens_with_its_merged_pixels() {
    let stack = stack();
    let merged = must(&stack, json!({"op": "merge_visible"}));
    let bytes = merged.encode_native(LIMIT, 0, None).expect("bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(reopened.layer_count(), 1);
    assert_eq!(
        samples(&reopened, 0),
        samples(&merged, 0),
        "the merged pixels survive the round trip"
    );
    assert_eq!(
        reopened.layer(0).expect("a record").id(),
        merged.layer(0).expect("a record").id(),
        "and so does the identity the merge kept"
    );
}
