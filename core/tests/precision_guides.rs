//! Phase 12E-1c: GUIDES and the RULER ORIGIN at high depth.
//!
//! Every rule checked here is `doc_guide.rs`'s, restated against a typed
//! document. Two independent yardsticks are used on purpose: the geometry
//! expectations are written out BY HAND in this file (the permutation a
//! quarter turn performs on a line, the crop shift, the resize scale), and
//! the same ops are ALSO run on a BYTE document carrying the same guides, so
//! a typed answer has to agree with a table nobody generated and with the
//! shipped byte implementation at once. The `LGDE` table is assembled here
//! byte by byte and compared with the writer's own output.
//!
//! The format's byte-level compatibility claims — that a plain document is
//! unchanged, that the new word sits inside 46 and the table before `LIDS`,
//! that reopen → resave is byte-identical — follow `precision_locks.rs`'s
//! shape, because 48 is 47's neighbour in the same slot.

use std::ffi::c_int;

use doctored_core::doc::DrDocument;
use doctored_core::doc_guide::{Guide, GuideOrientation};
use doctored_core::ffi_guide::{
    dr_precision_doc_add_guide, dr_precision_doc_clear_guides, dr_precision_doc_guide_count,
    dr_precision_doc_guide_id, dr_precision_doc_guide_orientation, dr_precision_doc_guide_position,
    dr_precision_doc_move_guide, dr_precision_doc_remove_guide, dr_precision_doc_ruler_origin,
    dr_precision_doc_set_ruler_origin,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 64;
const H: u32 = 48;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

/// `DrGuideOrientation`, mirrored here rather than read from the core.
const HORIZONTAL: GuideOrientation = GuideOrientation::Horizontal;
const VERTICAL: GuideOrientation = GuideOrientation::Vertical;

// ---------------------------------------------------------------- helpers --

fn typed(w: u32, h: u32) -> PrecisionDocument {
    let pixels = vec![[12345u16, 23456, 34567, 65535]; (w * h) as usize];
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

fn byte(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([200, 100, 50, 255])))
}

/// The whole list as `(orientation, position)` pairs, in the core's order.
fn pairs(guides: &[Guide]) -> Vec<(GuideOrientation, f64)> {
    guides.iter().map(|g| (g.orientation, g.position)).collect()
}

fn ids(guides: &[Guide]) -> Vec<u64> {
    guides.iter().map(|g| g.id).collect()
}

/// The three guides and the moved origin both depths start from.
const START: [(GuideOrientation, f64); 3] =
    [(VERTICAL, 10.5), (VERTICAL, 20.0), (HORIZONTAL, 7.25)];
const START_ORIGIN: (f64, f64) = (3.0, 4.0);

/// `START` plus `START_ORIGIN` on top of an EXISTING document, so a "these
/// bytes come back" comparison can be made against that same document's own
/// file — layer ids are persisted, and two freshly built documents carry
/// different ones.
fn seed(doc: &PrecisionDocument) -> PrecisionDocument {
    let mut doc = doc.clone();
    for (orientation, position) in START {
        doc = doc.add_guide(orientation, position).expect("a guide");
    }
    doc.set_ruler_origin(START_ORIGIN.0, START_ORIGIN.1)
        .expect("the origin moves")
}

fn seeded_typed() -> PrecisionDocument {
    seed(&typed(W, H))
}

fn seeded_byte() -> DrDocument {
    let mut doc = byte(W, H);
    for (orientation, position) in START {
        doc = doc.add_guide(orientation, position).expect("a guide");
    }
    doc.set_ruler_origin(START_ORIGIN.0, START_ORIGIN.1)
        .expect("the origin moves")
}

fn apply(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let edit = GeometryEdit::parse(&request.to_string()).expect("a geometry request");
    doc.geometry(&edit, LIMIT, 0, None)
        .expect("the edit runs")
        .expect("the edit changes something")
}

// ----------------------------------------------------- T1: the mutators --

#[test]
fn the_typed_mutators_answer_every_byte_refusal() {
    let base = typed(W, H);

    // ADD. The four refusals the byte method owns, one answer for all four.
    assert!(base.add_guide(VERTICAL, f64::NAN).is_none(), "non-finite");
    assert!(base.add_guide(VERTICAL, f64::INFINITY).is_none());
    assert!(base.add_guide(VERTICAL, -0.0001).is_none(), "below 0");
    assert!(
        base.add_guide(VERTICAL, f64::from(W) + 0.1).is_none(),
        "past the WIDTH, which is a vertical guide's extent"
    );
    assert!(
        base.add_guide(HORIZONTAL, f64::from(H) + 0.1).is_none(),
        "past the HEIGHT, which is a horizontal guide's extent"
    );
    // Both canvas edges are legal, at both orientations.
    assert!(base.add_guide(VERTICAL, 0.0).is_some());
    assert!(base.add_guide(VERTICAL, f64::from(W)).is_some());
    assert!(base.add_guide(HORIZONTAL, f64::from(H)).is_some());

    // The position is QUANTIZED to four decimals before it is stored, and a
    // value echoed back at full precision is then a duplicate.
    let quantized = base.add_guide(VERTICAL, 10.000_04).expect("added");
    assert_eq!(pairs(quantized.guides()), vec![(VERTICAL, 10.0)]);
    assert!(
        quantized.add_guide(VERTICAL, 10.0).is_none(),
        "a duplicate is not an edit"
    );
    assert!(
        quantized.add_guide(HORIZONTAL, 10.0).is_some(),
        "the same number at the OTHER orientation is a different line"
    );

    // MOVE.
    assert!(quantized.move_guide(9, 5.0).is_none(), "bad index");
    assert!(quantized.move_guide(0, f64::NAN).is_none(), "non-finite");
    assert!(quantized.move_guide(0, 900.0).is_none(), "out of canvas");
    assert!(
        quantized.move_guide(0, 10.0).is_none(),
        "the position it already has is not an edit"
    );
    let two = quantized.add_guide(VERTICAL, 30.0).expect("a second");
    assert!(
        two.move_guide(0, 30.0).is_none(),
        "onto another same-orientation guide is refused"
    );
    let moved = two.move_guide(0, 12.5).expect("moved");
    assert_eq!(
        pairs(moved.guides()),
        vec![(VERTICAL, 12.5), (VERTICAL, 30.0)]
    );
    assert_eq!(
        ids(moved.guides())[0],
        ids(two.guides())[0],
        "a move keeps the guide's identity"
    );

    // REMOVE and CLEAR.
    assert!(two.remove_guide(2).is_none(), "bad index");
    assert_eq!(
        pairs(two.remove_guide(0).expect("removed").guides()),
        vec![(VERTICAL, 30.0)]
    );
    assert!(
        base.clear_guides().is_none(),
        "clearing nothing is not an edit"
    );
    assert!(two.clear_guides().expect("cleared").guides().is_empty());

    // THE CAP: 1024 guides fit, the 1025th does not.
    let mut full = typed(W, H);
    for i in 0..1024u32 {
        full = full
            .add_guide(VERTICAL, f64::from(i) * 0.0625)
            .unwrap_or_else(|| panic!("guide {i} fits"));
    }
    assert_eq!(full.guides().len(), 1024);
    assert!(
        full.add_guide(VERTICAL, f64::from(W)).is_none(),
        "the 1025th is refused by the cap, not by the canvas"
    );

    // THE RULER ORIGIN.
    assert!(base.set_ruler_origin(0.0, 0.0).is_none(), "unchanged");
    assert!(base.set_ruler_origin(f64::NAN, 0.0).is_none());
    assert!(base.set_ruler_origin(0.0, f64::from(H) + 0.5).is_none());
    let origin = base.set_ruler_origin(3.000_04, 4.000_06).expect("moved");
    assert_eq!(origin.ruler_origin(), (3.0, 4.0001), "quantized to four");
    assert!(
        origin.set_ruler_origin(3.0, 4.0001).is_none(),
        "the value it reports back is refused as unchanged"
    );
}

// ------------------------------------------------- T1b: the ten exports --

/// The ten typed exports, driven as the host drives them: every read, every
/// mutator, one refusal. The model rules are T1's; this says the C surface
/// is wired to them. NULL handling for the same ten is in `doc_tests`'s
/// null-safety sweep.
#[test]
fn the_typed_guide_exports_add_read_and_clear() {
    // `DrGuideOrientation`, mirrored from the header like the pair above.
    const C_HORIZONTAL: c_int = 0;
    const C_VERTICAL: c_int = 1;

    let base = typed(W, H);
    unsafe {
        assert_eq!(dr_precision_doc_guide_count(&base), 0);

        let added = dr_precision_doc_add_guide(&base, C_VERTICAL, 10.5);
        assert!(!added.is_null(), "a legal guide is added");
        let added = Box::from_raw(added);
        assert_eq!(dr_precision_doc_guide_count(&*added), 1);
        assert_ne!(dr_precision_doc_guide_id(&*added, 0), 0, "an id is minted");
        assert_eq!(dr_precision_doc_guide_orientation(&*added, 0), C_VERTICAL);
        assert_eq!(dr_precision_doc_guide_position(&*added, 0), 10.5);
        assert_eq!(
            dr_precision_doc_guide_orientation(&*added, 1),
            -1,
            "past the end is the sentinel, not a panic"
        );

        // A refusal the model owns travels out as NULL.
        assert!(
            dr_precision_doc_add_guide(&*added, C_VERTICAL, f64::from(W) + 1.0).is_null(),
            "off the canvas is refused"
        );

        let both = dr_precision_doc_add_guide(&*added, C_HORIZONTAL, 7.25);
        assert!(!both.is_null(), "a second legal guide is added");
        let both = Box::from_raw(both);
        assert_eq!(dr_precision_doc_guide_count(&*both), 2);
        assert_eq!(dr_precision_doc_guide_orientation(&*both, 0), C_HORIZONTAL);
        assert_eq!(dr_precision_doc_guide_position(&*both, 0), 7.25);

        let moved = dr_precision_doc_move_guide(&*both, 0, 8.0);
        assert!(!moved.is_null(), "a legal move is an edit");
        let moved = Box::from_raw(moved);
        assert_eq!(dr_precision_doc_guide_position(&*moved, 0), 8.0);

        let fewer = dr_precision_doc_remove_guide(&*moved, 0);
        assert!(!fewer.is_null(), "removing a live guide is an edit");
        let fewer = Box::from_raw(fewer);
        assert_eq!(dr_precision_doc_guide_count(&*fewer), 1);
        assert_eq!(dr_precision_doc_guide_position(&*fewer, 0), 10.5);

        // The origin, written into the caller's two doubles.
        let mut origin = [0.0f64; 2];
        assert!(dr_precision_doc_ruler_origin(&*fewer, origin.as_mut_ptr()));
        assert_eq!(origin, [0.0, 0.0]);
        let shifted = dr_precision_doc_set_ruler_origin(&*fewer, 3.0, 4.0);
        assert!(!shifted.is_null(), "moving the origin is an edit");
        let shifted = Box::from_raw(shifted);
        assert!(dr_precision_doc_ruler_origin(
            &*shifted,
            origin.as_mut_ptr()
        ));
        assert_eq!(origin, [3.0, 4.0]);

        let cleared = dr_precision_doc_clear_guides(&*shifted);
        assert!(!cleared.is_null(), "clearing a non-empty list is an edit");
        let cleared = Box::from_raw(cleared);
        assert_eq!(dr_precision_doc_guide_count(&*cleared), 0);
        assert!(
            dr_precision_doc_clear_guides(&*cleared).is_null(),
            "clearing an empty list is no edit"
        );
        assert!(
            dr_precision_doc_ruler_origin(&*cleared, origin.as_mut_ptr()),
            "the origin outlives the guides"
        );
        assert_eq!(origin, [3.0, 4.0]);
    }
}

// --------------------------------------------- T2: the geometry table --

/// One hand-written row of the geometry table: the request, the guides it
/// must produce, and the origin it must produce.
type GeometryRow = (Value, Vec<(GuideOrientation, f64)>, (f64, f64));

/// The whole table — written out by hand from the geometry itself, on the
/// 64x48 canvas the fixtures use.
fn geometry_table() -> Vec<GeometryRow> {
    vec![
        // Crop to x in [4, 44), y in [2, 32): every guide shifts by (-4, -2)
        // and all three survive.
        (
            json!({"op": "crop", "x": 4, "y": 2, "width": 40, "height": 30}),
            vec![(HORIZONTAL, 5.25), (VERTICAL, 6.5), (VERTICAL, 16.0)],
            (0.0, 2.0),
        ),
        // Rotate 90 clockwise on 64x48 -> 48x64. A vertical guide at x
        // becomes a horizontal one at x; a horizontal one at y becomes a
        // vertical one at 48 - y. The origin (3, 4) -> (48 - 4, 3).
        (
            json!({"op": "rotate90"}),
            vec![
                (HORIZONTAL, 10.5),
                (HORIZONTAL, 20.0),
                (VERTICAL, 48.0 - 7.25),
            ],
            (44.0, 3.0),
        ),
        // Rotate 180: each guide keeps its orientation and reflects on its
        // own axis. The origin reflects on both.
        (
            json!({"op": "rotate180"}),
            vec![
                (HORIZONTAL, 48.0 - 7.25),
                (VERTICAL, 64.0 - 20.0),
                (VERTICAL, 64.0 - 10.5),
            ],
            (61.0, 44.0),
        ),
        // Rotate 270: a vertical guide at x becomes horizontal at 64 - x; a
        // horizontal one at y becomes vertical at y.
        (
            json!({"op": "rotate270"}),
            vec![
                (HORIZONTAL, 64.0 - 20.0),
                (HORIZONTAL, 64.0 - 10.5),
                (VERTICAL, 7.25),
            ],
            (4.0, 61.0),
        ),
        (
            json!({"op": "flip_horizontal"}),
            vec![
                (HORIZONTAL, 7.25),
                (VERTICAL, 64.0 - 20.0),
                (VERTICAL, 64.0 - 10.5),
            ],
            (61.0, 4.0),
        ),
        (
            json!({"op": "flip_vertical"}),
            vec![
                (HORIZONTAL, 48.0 - 7.25),
                (VERTICAL, 10.5),
                (VERTICAL, 20.0),
            ],
            (3.0, 44.0),
        ),
        // Image Size to 32x96: fx = 0.5, fy = 2, and NOTHING is rounded.
        (
            json!({"op": "resize", "width": 32, "height": 96, "filter": "bilinear"}),
            vec![(HORIZONTAL, 14.5), (VERTICAL, 5.25), (VERTICAL, 10.0)],
            (1.5, 8.0),
        ),
    ]
}

#[test]
fn typed_canvas_geometry_moves_the_chrome_exactly_as_the_byte_ops_do() {
    for (request, expected, expected_origin) in geometry_table() {
        let out = apply(&seeded_typed(), request.clone());
        assert_eq!(
            pairs(out.guides()),
            expected,
            "{request}: the hand-written guide table"
        );
        assert_eq!(
            out.ruler_origin(),
            expected_origin,
            "{request}: the hand-written origin"
        );

        // …and the SAME ops on a byte document carrying the same guides.
        let base = seeded_byte();
        let twin = match request["op"].as_str().unwrap() {
            "crop" => base.crop(4, 2, 40, 30).expect("byte crop"),
            "rotate90" => base.rotate90().unwrap(),
            "rotate180" => base.rotate180().unwrap(),
            "rotate270" => base.rotate270().unwrap(),
            "flip_horizontal" => base.flip_horizontal().unwrap(),
            "flip_vertical" => base.flip_vertical().unwrap(),
            "resize" => base
                .resize(32, 96, image::imageops::FilterType::Triangle)
                .expect("byte resize"),
            other => panic!("unhandled op {other}"),
        };
        assert_eq!(
            pairs(&twin.guides),
            pairs(out.guides()),
            "{request}: the typed answer is the byte answer"
        );
        assert_eq!(twin.ruler_origin, out.ruler_origin(), "{request}: origin");
    }
}

#[test]
fn a_per_layer_transform_leaves_the_canvas_chrome_alone() {
    let base = seeded_typed();
    let out = apply(
        &base,
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": [1, 0, 0, 1, 3, -2], "filter": "bilinear"}),
    );
    assert_eq!(
        pairs(out.guides()),
        pairs(base.guides()),
        "moving ONE layer moves nothing about the canvas the guides cross"
    );
    assert_eq!(ids(out.guides()), ids(base.guides()), "ids travel too");
    assert_eq!(out.ruler_origin(), base.ruler_origin());
}

#[test]
fn a_crop_that_moves_only_a_guide_is_still_an_edit() {
    // A 1:1 crop of the whole canvas moves no pixel and changes no
    // dimension, but it DOES shift nothing and drop nothing — so it is a
    // no-op and answers None. Shifting the window by one pixel moves the
    // guides, and that alone must register.
    //
    // Note the window here also narrows 64 -> 63, so `changed` is already
    // true from the dimension clause: no canvas op that keeps BOTH
    // dimensions and still moves a guide is reachable today (a crop that
    // shifts always changes a dimension, and typed canvas resize does not
    // exist — 12F/P7-28). This test therefore pins the resulting guide
    // POSITIONS, not the `guides != self.guides` term on its own.
    let base = seeded_typed();
    let edit = GeometryEdit::parse(
        &json!({"op": "crop", "x": 1, "y": 0, "width": 63, "height": 48}).to_string(),
    )
    .unwrap();
    let out = base
        .geometry(&edit, LIMIT, 0, None)
        .expect("runs")
        .expect("a crop is an edit");
    assert_eq!(
        pairs(out.guides()),
        vec![(HORIZONTAL, 7.25), (VERTICAL, 9.5), (VERTICAL, 19.0)]
    );
}

// ------------------------------------------------------- T3: the table --

fn word_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// The `LGDE` table this file assembles from the model alone.
fn hand_table(origin: (f64, f64), guides: &[(u8, f64)]) -> Vec<u8> {
    let mut table = b"LGDE".to_vec();
    table.extend(1u32.to_le_bytes());
    table.extend(origin.0.to_le_bytes());
    table.extend(origin.1.to_le_bytes());
    table.extend((guides.len() as u32).to_le_bytes());
    for (orientation, position) in guides {
        table.push(*orientation);
        table.extend(position.to_le_bytes());
    }
    table
}

#[test]
fn guides_survive_the_native_round_trip_byte_for_byte() {
    let base = typed(W, H);
    let plain = base.encode_native(LIMIT, 0, None).expect("plain bytes");
    assert!(
        !plain.windows(4).any(|w| w == b"LGDE"),
        "a document with no guide and the origin at (0,0) writes no table"
    );
    assert_eq!(&plain[..4], b"DRDC");
    assert_eq!(word_at(&plain, 4), 46, "46 is still the FIRST word");
    assert_ne!(word_at(&plain, 8), 48, "and 48 is absent");

    let seeded = seed(&base);
    let bytes = seeded.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(word_at(&bytes, 4), 46, "46 stays FIRST");
    assert_eq!(
        word_at(&bytes, 8),
        48,
        "48 sits INSIDE the identity wrapper"
    );

    let tail = bytes.len() - (12 + 8 * seeded.layer_count());
    assert_eq!(&bytes[tail..tail + 4], b"LIDS", "LIDS stays LAST");
    let table = hand_table(START_ORIGIN, &[(0, 7.25), (1, 10.5), (1, 20.0)]);
    let at = bytes
        .windows(4)
        .position(|w| w == b"LGDE")
        .expect("an LGDE table");
    assert_eq!(
        &bytes[at..at + table.len()],
        &table[..],
        "the table this file assembled is the one the writer emitted"
    );
    assert_eq!(at + table.len(), tail, "LGDE sits immediately before LIDS");

    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(pairs(reopened.guides()), pairs(seeded.guides()));
    assert_eq!(reopened.ruler_origin(), START_ORIGIN);
    assert_ne!(
        ids(reopened.guides()),
        ids(seeded.guides()),
        "ids are NOT persisted: a reopen mints fresh ones"
    );
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes,
        "reopen then resave is byte-identical"
    );

    // Clearing the last guide with the origin back at (0,0) restores the
    // 12E-1b writer's exact bytes.
    let cleared = seeded.clear_guides().expect("cleared");
    let cleared = cleared.set_ruler_origin(0.0, 0.0).expect("origin home");
    assert_eq!(
        cleared
            .encode_native(LIMIT, 0, None)
            .expect("cleared bytes"),
        plain,
        "the plain file comes back exactly"
    );

    // An origin move ALONE writes the table, with a zero count.
    let origin_only = base.set_ruler_origin(2.0, 1.0).expect("moved");
    let bytes = origin_only.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(word_at(&bytes, 8), 48);
    let at = bytes.windows(4).position(|w| w == b"LGDE").expect("table");
    assert_eq!(
        &bytes[at..at + hand_table((2.0, 1.0), &[]).len()],
        &hand_table((2.0, 1.0), &[])[..]
    );
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(reopened.ruler_origin(), (2.0, 1.0));
    assert!(reopened.guides().is_empty());
}

#[test]
fn the_guide_word_sits_beside_the_lock_word_inside_the_identity_wrapper() {
    let locked = seeded_typed()
        .with_layer_locks(0, 2)
        .expect("a lock edit")
        .expect("the lock changes something");
    let bytes = locked.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(word_at(&bytes, 4), 46, "46 first");
    assert_eq!(word_at(&bytes, 8), 47, "then the lock word");
    assert_eq!(word_at(&bytes, 12), 48, "then the guide word");

    let llnk = bytes.windows(4).position(|w| w == b"LLNK").expect("LLNK");
    let lgde = bytes.windows(4).position(|w| w == b"LGDE").expect("LGDE");
    let lids = bytes.windows(4).rposition(|w| w == b"LIDS").expect("LIDS");
    assert!(llnk < lgde && lgde < lids, "… -> LLNK -> LGDE -> LIDS");

    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(pairs(reopened.guides()), pairs(locked.guides()));
    assert_eq!(reopened.layer(0).expect("a record").locks(), 2);
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes,
        "reopen then resave is byte-identical with both words present"
    );
}

// ---------------------------------------------------- T4: crafted files --

#[test]
fn a_crafted_guide_table_refuses_the_open() {
    let good = seeded_typed().encode_native(LIMIT, 0, None).expect("bytes");
    let at = good.windows(4).position(|w| w == b"LGDE").expect("LGDE");

    let refuse = |bytes: &[u8], what: &str| -> String {
        match PrecisionDocument::parse_native(bytes, LIMIT, 0, None) {
            Err(message) => message,
            Ok(_) => panic!("{what} must refuse the open"),
        }
    };

    let mut tag = good.clone();
    tag[at..at + 4].copy_from_slice(b"XXXX");
    assert!(refuse(&tag, "a bad tag").contains("native guide schema"));

    let mut schema = good.clone();
    schema[at + 4..at + 8].copy_from_slice(&2u32.to_le_bytes());
    assert!(refuse(&schema, "a bad schema").contains("native guide schema"));

    let mut count = good.clone();
    count[at + 24..at + 28].copy_from_slice(&2000u32.to_le_bytes());
    assert!(refuse(&count, "a count past the cap").contains("native guide count"));

    let mut orientation = good.clone();
    orientation[at + 28] = 9;
    assert!(
        refuse(&orientation, "an unknown orientation").contains("native guide orientation"),
        "the orientation byte is a structural claim and is REFUSED"
    );

    // A POSITION, by contrast, is repaired: out of canvas clamps and
    // non-finite drops that guide.
    let mut position = good.clone();
    position[at + 29..at + 37].copy_from_slice(&5000.0f64.to_le_bytes());
    let opened = PrecisionDocument::parse_native(&position, LIMIT, 0, None)
        .expect("a nonsense position is repaired, not refused");
    assert_eq!(
        pairs(opened.guides())[0],
        (HORIZONTAL, f64::from(H)),
        "clamped to the canvas edge"
    );

    let mut dropped = good.clone();
    dropped[at + 29..at + 37].copy_from_slice(&f64::NAN.to_le_bytes());
    let opened = PrecisionDocument::parse_native(&dropped, LIMIT, 0, None).expect("repaired");
    assert_eq!(opened.guides().len(), 2, "the non-finite guide is dropped");
}

#[test]
fn an_unknown_word_after_the_identity_wrapper_refuses_by_name() {
    let mut bytes = seeded_typed().encode_native(LIMIT, 0, None).expect("bytes");
    bytes[8..12].copy_from_slice(&49u32.to_le_bytes());
    let message = match PrecisionDocument::parse_native(&bytes, LIMIT, 0, None) {
        Err(message) => message,
        Ok(_) => panic!("an unknown body word must refuse the open"),
    };
    assert!(
        message.contains(
            "not a supported version-11/12/13/15/17/19/20/22/24/25 high-depth DRDC document"
        ),
        "{message}"
    );
}

// -------------------------------------------------- T5/T7: the carriage --

#[test]
fn the_chrome_crosses_every_depth_conversion_and_profile_edit() {
    // BYTE -> TYPED.
    let promoted = seeded_byte()
        .promote_flattened(LIMIT, 0, None)
        .expect("promotion");
    assert_eq!(
        pairs(promoted.guides()),
        vec![(HORIZONTAL, 7.25), (VERTICAL, 10.5), (VERTICAL, 20.0)]
    );
    assert_eq!(promoted.ruler_origin(), START_ORIGIN);

    // TYPED -> BYTE.
    let reduced = seeded_typed()
        .downconvert(PrecisionDither::None, LIMIT, 0, None)
        .expect("downconversion");
    assert_eq!(
        pairs(&reduced.guides),
        vec![(HORIZONTAL, 7.25), (VERTICAL, 10.5), (VERTICAL, 20.0)]
    );
    assert_eq!(reduced.ruler_origin, START_ORIGIN);

    // The plan JSON says so, in PRESERVED rather than in DISCARDED.
    let plan: Value = seeded_typed().downconvert_plan().expect("plan").json();
    let preserved: Vec<String> = serde_json::from_value(plan["preserved"].clone()).unwrap();
    let discarded: Vec<String> = serde_json::from_value(plan["discarded"].clone()).unwrap();
    for key in ["guides", "ruler_origin"] {
        assert!(preserved.iter().any(|v| v == key), "{key} is preserved");
        assert!(
            !discarded.iter().any(|v| v == key),
            "{key} is not discarded"
        );
    }

    // ASSIGN PROFILE moves no pixel and no canvas.
    let assigned = seeded_typed()
        .assign_profile(&IccProfile::display_p3(), LIMIT, 0, None)
        .expect("assignment runs")
        .expect("the profile changes");
    assert_eq!(
        pairs(assigned.guides()),
        pairs(seeded_typed().guides()),
        "an assign carries the chrome"
    );
    assert_eq!(assigned.ruler_origin(), START_ORIGIN);

    // …and so does a CONVERT.
    let converted = seeded_typed()
        .convert_to_profile(&IccProfile::display_p3(), LIMIT, 0, None)
        .expect("conversion runs")
        .expect("the profile changes");
    assert_eq!(converted.ruler_origin(), START_ORIGIN);
    assert_eq!(converted.guides().len(), 3);
}

// -------------------------------------------------------- T6: at float --

fn float_fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    Some(PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).expect("the fixture"))
}

#[test]
fn a_float_document_takes_the_mutators_and_round_trips_the_table() {
    let Some(base) = float_fixture() else {
        return;
    };
    assert!(base.is_float(), "the fixture is a 32-bit float document");
    let (w, h) = base.dimensions();

    let seeded = base
        .add_guide(VERTICAL, f64::from(w) / 4.0)
        .expect("a guide")
        .add_guide(HORIZONTAL, f64::from(h) / 2.0)
        .expect("a second")
        .set_ruler_origin(1.0, 2.0)
        .expect("the origin moves");
    assert_eq!(seeded.guides().len(), 2);

    let bytes = seeded.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(word_at(&bytes, 4), 46, "46 first at float too");
    assert_eq!(word_at(&bytes, 8), 48);
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert!(reopened.is_float());
    assert_eq!(pairs(reopened.guides()), pairs(seeded.guides()));
    assert_eq!(reopened.ruler_origin(), (1.0, 2.0));
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes,
        "reopen then resave is byte-identical at float"
    );

    // The deliberate float -> u16 conversion carries both.
    let integer = seeded.float_to_u16(LIMIT, 0, None).expect("conversion");
    assert!(!integer.is_float());
    assert_eq!(pairs(integer.guides()), pairs(seeded.guides()));
    assert_eq!(integer.ruler_origin(), (1.0, 2.0));

    // Clearing restores the plain float bytes.
    let plain = base.encode_native(LIMIT, 0, None).expect("plain");
    let cleared = seeded
        .clear_guides()
        .expect("cleared")
        .set_ruler_origin(0.0, 0.0)
        .expect("origin home");
    assert_eq!(
        cleared
            .encode_native(LIMIT, 0, None)
            .expect("cleared bytes"),
        plain
    );
}
