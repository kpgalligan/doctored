//! Phase 12E-1b: TYPED layer locks and links.
//!
//! Every rule checked here is `doc_lock.rs`'s and `doc_align.rs`'s, restated
//! against typed records. The lock CONSTANTS and the `DrEditKind` codes are
//! written out in this file rather than read out of the core, so a change to
//! either number has to be made twice; the refusals are checked by their
//! SENTENCES, because that is what a host shows; and the frozen-alpha
//! arithmetic is compared against samples this file composes itself.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_precision_inspect::{
    dr_precision_doc_lock_block, dr_precision_doc_lock_owner,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// `DrLockFlags`, restated here rather than read out of the core.
const LOCK_TRANSPARENCY: u32 = 1;
const LOCK_PIXELS: u32 = 2;
const LOCK_POSITION: u32 = 4;
const LOCK_ALL: u32 = 7;

/// `DrEditKind`, restated for the same reason.
const KIND_PIXELS: i32 = 0;
const KIND_POSITION: i32 = 1;
const KIND_MASK: i32 = 2;
const KIND_MASK_APPLY: i32 = 3;
const KIND_MERGE: i32 = 4;

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

fn edit_with(
    document: &PrecisionDocument,
    request: Value,
    samples: &[u16],
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            request.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
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

fn edit(
    document: &PrecisionDocument,
    request: Value,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    edit_with(document, request, &[])
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
        Ok(Some(_)) => panic!("{request} must be refused"),
        Ok(None) => panic!("{request} must be refused, not answered as a no-op"),
    }
}

fn info(document: &PrecisionDocument) -> Vec<Value> {
    let value: Value =
        serde_json::from_str(&document.info().expect("info").to_string()).expect("info JSON");
    value["layers"].as_array().expect("layer rows").clone()
}

fn samples(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    document
        .layer_source(index)
        .expect("a raster record")
        .to_rgba16()
        .expect("u16 storage")
}

// ------------------------------------------------------------- fixtures --

fn image(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(2, 2, pixels, IccProfile::srgb(), None).expect("an image")
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

/// One raster whose four pixels carry four different alphas — opaque, half,
/// quarter and EMPTY — so the frozen-alpha rule can be checked at each.
fn fixture() -> PrecisionDocument {
    document(vec![Layer::raster(
        "Target",
        image(vec![
            [40000, 30000, 20000, 65535],
            [40000, 30000, 20000, 32768],
            [40000, 30000, 20000, 16384],
            [40000, 30000, 20000, 0],
        ]),
        (0, 0),
        1.0,
        true,
    )
    .unwrap()])
}

/// Two rasters and a group holding a third, so the POSITION walk over a
/// group's descendants has something to find.
fn nested() -> PrecisionDocument {
    let inner =
        Layer::raster("Inner", image(vec![[1, 2, 3, 65535]; 4]), (0, 0), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Group", vec![inner], 1.0, true).unwrap();
    let outer =
        Layer::raster("Outer", image(vec![[4, 5, 6, 65535]; 4]), (0, 0), 1.0, true).unwrap();
    document(vec![group, outer])
}

/// A full-canvas coverage plane at `value`.
fn coverage(value: u16) -> Vec<u16> {
    vec![value; 4]
}

// --------------------------------------------------------------- lock_block --

#[test]
fn lock_block_answers_the_byte_rules_for_every_kind() {
    let base = fixture();
    // Unlocked: nothing blocks anything.
    for kind in [
        KIND_PIXELS,
        KIND_POSITION,
        KIND_MASK,
        KIND_MASK_APPLY,
        KIND_MERGE,
    ] {
        assert_eq!(
            unsafe { dr_precision_doc_lock_block(&base, 0, kind) },
            0,
            "an unlocked record blocks nothing"
        );
    }

    // (flags, pixels, position, mask, mask apply, merge)
    let table: &[(u32, u32, u32, u32, u32, u32)] = &[
        (
            LOCK_TRANSPARENCY,
            0,
            0,
            0,
            LOCK_TRANSPARENCY,
            LOCK_TRANSPARENCY,
        ),
        (LOCK_PIXELS, LOCK_PIXELS, 0, 0, 0, LOCK_PIXELS),
        (LOCK_POSITION, 0, LOCK_POSITION, 0, 0, 0),
        (
            LOCK_ALL,
            LOCK_PIXELS,
            LOCK_POSITION,
            LOCK_ALL,
            LOCK_ALL,
            LOCK_PIXELS | LOCK_TRANSPARENCY,
        ),
    ];
    for &(flags, pixels, position, mask, apply, merge) in table {
        let locked = must(
            &base,
            json!({"op": "layer_lock", "layer": 0, "locks": flags}),
        );
        let block = |kind| unsafe { dr_precision_doc_lock_block(&*locked, 0, kind) };
        assert_eq!(block(KIND_PIXELS), pixels, "{flags}: pixels");
        assert_eq!(block(KIND_POSITION), position, "{flags}: position");
        assert_eq!(block(KIND_MASK), mask, "{flags}: mask");
        assert_eq!(block(KIND_MASK_APPLY), apply, "{flags}: mask apply");
        assert_eq!(block(KIND_MERGE), merge, "{flags}: merge");
    }

    // An out-of-range index and an unrecognized kind both answer 0.
    assert_eq!(
        unsafe { dr_precision_doc_lock_block(&base, 9, KIND_PIXELS) },
        0
    );
    assert_eq!(unsafe { dr_precision_doc_lock_block(&base, 0, 99) }, 0);
    assert_eq!(
        unsafe { dr_precision_doc_lock_block(ptr::null(), 0, KIND_PIXELS) },
        0,
        "and so does a NULL document"
    );
    let mut out = 7usize;
    assert!(!unsafe { dr_precision_doc_lock_owner(ptr::null(), 0, LOCK_ALL, &mut out) });
    assert_eq!(out, 7, "nothing is written for a NULL document");
}

#[test]
fn a_position_locked_descendant_blocks_moving_its_group() {
    let nested = nested();
    // Record 0 is Inner, record 1 is the Group, record 2 is Outer.
    let locked = must(
        &nested,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_POSITION}),
    );
    assert_eq!(
        unsafe { dr_precision_doc_lock_block(&*locked, 1, KIND_POSITION) },
        LOCK_POSITION,
        "moving a group moves its children, so the child's bit answers"
    );
    assert_eq!(
        unsafe { dr_precision_doc_lock_block(&*locked, 2, KIND_POSITION) },
        0,
        "an unrelated sibling is unaffected"
    );
    assert_eq!(
        unsafe { dr_precision_doc_lock_block(&*locked, 1, KIND_PIXELS) },
        0,
        "and only the POSITION kind walks the subtree"
    );
}

// ------------------------------------------------------------ the setter --

#[test]
fn setting_the_locks_masks_reserved_bits_and_honours_the_no_op_rule() {
    let base = fixture();
    let locked = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": 0xFFFF_FFFFu32}),
    );
    assert_eq!(
        info(&locked)[0]["locks"],
        json!(LOCK_ALL),
        "bits 3..31 are reserved and masked off wherever a value enters"
    );
    assert!(
        matches!(
            edit(
                &locked,
                json!({"op": "layer_lock", "layer": 0, "locks": LOCK_ALL})
            ),
            Ok(None)
        ),
        "the flags the record already carries are the exact no-op"
    );
    assert!(
        refusal(&base, json!({"op": "layer_lock", "layer": 9, "locks": 1}))
            .contains("index is out of range"),
        "an out-of-range index is refused by name"
    );
    assert_eq!(info(&base)[0]["locks"], json!(0), "the input is untouched");
}

// ---------------------------------------------------------- enforcement --

#[test]
fn lock_pixels_refuses_a_paint_and_lock_transparency_freezes_the_alpha() {
    let base = fixture();
    let before = samples(&base, 0);
    let paint = json!({"op": "paint", "layer": 0, "opacity": 1.0,
                       "color": [1.0, 0.0, 0.0, 1.0], "color_space": "document"});

    // LOCK PIXELS refuses outright, by name.
    let pixels = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_PIXELS}),
    );
    let message = match edit_with(&pixels, paint.clone(), &coverage(65535)) {
        Err(message) => message,
        Ok(_) => panic!("a pixel-locked record must refuse a paint"),
    };
    assert!(
        message.contains("is locked (pixels)"),
        "the refusal names the lock: {message}"
    );

    // LOCK TRANSPARENCY admits the paint and restores the ALPHA channel.
    let frozen = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_TRANSPARENCY}),
    );
    let painted = edit_with(&frozen, paint, &coverage(65535))
        .expect("the paint is admitted")
        .expect("it changes something");
    let after = samples(&painted, 0);
    for (i, (before, after)) in before.iter().zip(&after).enumerate() {
        assert_eq!(
            after[3], before[3],
            "pixel {i}: the alpha channel is frozen"
        );
        if before[3] == 0 {
            // A pixel the record holds at zero alpha is not part of the
            // picture, so its COLOUR comes back too — that is what lets an
            // edit landing only in the empty area be a byte-exact no-op.
            assert_eq!(after, before, "pixel {i}: an empty pixel is untouched");
        } else {
            assert_ne!(after[0], before[0], "pixel {i}: the colour did move");
        }
    }
}

#[test]
fn an_erase_on_a_transparency_locked_record_is_an_exact_no_op() {
    // An eraser can only REMOVE coverage, and coverage is frozen — so the
    // restore leaves the record byte-identical and the op answers "nothing
    // changed" rather than minting an undo step.
    let base = fixture();
    let frozen = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_TRANSPARENCY}),
    );
    let result = edit_with(
        &frozen,
        json!({"op": "erase", "layer": 0, "opacity": 1.0}),
        &coverage(65535),
    );
    assert!(
        matches!(result, Ok(None)),
        "a frozen-alpha erase is the exact no-op"
    );
}

#[test]
fn lock_position_refuses_a_transform_and_lock_transparency_does_not() {
    let base = fixture();
    let scale = json!({"op": "affine", "layer": 0, "target": "pixels",
                       "matrix": [2.0, 0.0, 0.0, 2.0, 0.0, 0.0], "filter": "bilinear"});

    let position = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_POSITION}),
    );
    let message = refusal(&position, scale.clone());
    assert!(
        message.contains("is locked (position)"),
        "a transform is a POSITION edit: {message}"
    );

    // A transparency-locked record is still transformable — a transform
    // resamples the alpha with everything else, so the freeze has no meaning
    // there, and the buffer it produces has moved, which is exactly the case
    // the frozen-alpha rule refuses when it IS a Pixels edit.
    let frozen = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_TRANSPARENCY}),
    );
    let scaled = must(&frozen, scale);
    assert_eq!(
        info(&scaled)[0]["dimensions"],
        json!([4, 4]),
        "the transform ran"
    );
}

#[test]
fn a_mask_edit_is_blocked_only_by_lock_all_and_an_apply_also_by_transparency() {
    let base = fixture();
    let add = json!({"op": "set_mask", "layer": 0, "width": 2, "height": 2});

    // Lock PIXELS does not block a mask edit: a pixel-locked layer's mask
    // can still be painted.
    let pixels = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_PIXELS}),
    );
    let masked = edit_with(&pixels, add.clone(), &coverage(32768))
        .expect("a pixel lock admits a mask edit")
        .expect("it changes something");
    assert_ne!(info(&masked)[0]["mask"], Value::Null);

    // Lock ALL is the one spelling that freezes the mask too.
    let all = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_ALL}),
    );
    let message = match edit_with(&all, add, &coverage(32768)) {
        Err(message) => message,
        Ok(_) => panic!("Lock All must refuse a mask edit"),
    };
    assert!(message.contains("is locked (all)"), "{message}");

    // APPLYING a mask writes the layer's ALPHA, so Lock Transparency refuses
    // it BY NAME rather than silently un-applying it.
    let frozen = must(
        &masked,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_TRANSPARENCY}),
    );
    let message = refusal(&frozen, json!({"op": "apply_mask", "layer": 0}));
    assert!(
        message.contains("is locked (transparency)"),
        "an apply is named, not silently dropped: {message}"
    );
    // …while an unlocked record applies it.
    let applied = must(&masked, json!({"op": "apply_mask", "layer": 0}));
    assert_eq!(info(&applied)[0]["mask"], Value::Null);
}

#[test]
fn the_lock_exempt_edits_still_run_on_a_fully_locked_record() {
    // Locks never block a rename, a visibility change, an opacity change, a
    // blend mode, a colour label, a clipping flag or a structural edit.
    let base = nested();
    let locked = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_ALL}),
    );
    for request in [
        json!({"op": "rename_layer", "layer": 0, "name": "Renamed"}),
        json!({"op": "layer_visible", "layer": 0, "visible": false}),
        json!({"op": "layer_label", "layer": 0, "label": 2}),
        json!({"op": "layer_blend", "version": 1, "layer": 0, "blend_mode": 1}),
        json!({"op": "remove_layers", "layers": [0]}),
        json!({"op": "arrange_layer", "layer": 1, "how": "front"}),
        json!({"op": "resize", "width": 4, "height": 4, "filter": "bilinear"}),
        json!({"op": "crop", "x": 0, "y": 0, "width": 1, "height": 1}),
        json!({"op": "rotate90"}),
        json!({"op": "flip_horizontal"}),
    ] {
        assert!(
            edit(&locked, request.clone()).is_ok(),
            "{request} must not be blocked by a lock"
        );
    }
}

// ------------------------------------------------------------ the links --

#[test]
fn link_layers_takes_the_smallest_unused_id_and_prunes_a_lone_member() {
    let base = nested();
    let linked = must(&base, json!({"op": "link_layers", "layers": [1, 2]}));
    let rows = info(&linked);
    assert_eq!(rows[1]["link"], json!(1), "the smallest unused id is 1");
    assert_eq!(rows[2]["link"], json!(1));
    assert_eq!(rows[0]["link"], json!(0), "and nothing else is linked");

    // Re-linking the SAME set is the exact no-op: the ids are cleared before
    // the next one is computed, so it comes back as the same number.
    assert!(
        matches!(
            edit(&linked, json!({"op": "link_layers", "layers": [1, 2]})),
            Ok(None)
        ),
        "re-linking the same set changes nothing"
    );

    // A second group takes 2, and unlinking one member of a PAIR clears both
    // — a link group of one is not a link.
    let second = must(&linked, json!({"op": "link_layers", "layers": [0, 1]}));
    let rows = info(&second);
    assert_eq!(rows[0]["link"], json!(2));
    assert_eq!(rows[1]["link"], json!(2), "record 1 moved to the new group");
    assert_eq!(
        rows[2]["link"],
        json!(0),
        "its old partner was left alone, so its id was pruned"
    );

    let unlinked = must(&second, json!({"op": "unlink_layers", "layers": [0]}));
    let rows = info(&unlinked);
    assert_eq!(rows[0]["link"], json!(0));
    assert_eq!(rows[1]["link"], json!(0), "the lone survivor is pruned too");
    assert!(
        matches!(
            edit(&unlinked, json!({"op": "unlink_layers", "layers": [0]})),
            Ok(None)
        ),
        "unlinking what is not linked changes nothing"
    );
}

#[test]
fn the_link_refusals_are_named() {
    let base = nested();
    assert!(
        refusal(&base, json!({"op": "link_layers", "layers": [1]})).contains("at least two layers"),
        "a link group of one is refused by name"
    );
    assert!(
        refusal(&base, json!({"op": "link_layers", "layers": [0, 9]}))
            .contains("index is out of range"),
        "an out-of-range index is refused by name"
    );
    assert!(
        refusal(&base, json!({"op": "unlink_layers", "layers": []})).contains("at least one layer"),
        "an empty unlink set is refused by name"
    );
}

// --------------------------------------------------------------- native --

#[test]
fn locks_and_links_survive_the_native_round_trip_byte_for_byte() {
    let base = nested();
    let plain = base.encode_native(LIMIT, 0, None).expect("plain bytes");
    assert!(
        !plain.windows(4).any(|w| w == b"LLNK"),
        "a document with neither locks nor links writes no table"
    );
    // 46 is still the FIRST word and LIDS still the LAST table.
    assert_eq!(&plain[..4], b"DRDC");
    assert_eq!(
        u32::from_le_bytes(plain[4..8].try_into().unwrap()),
        46,
        "the identity word stays outermost"
    );

    let locked = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_ALL}),
    );
    let locked = must(&locked, json!({"op": "link_layers", "layers": [1, 2]}));
    let bytes = locked.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(&bytes[..4], b"DRDC");
    assert_eq!(
        u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        46,
        "46 stays FIRST even with the new word present"
    );
    assert_eq!(
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        47,
        "47 sits INSIDE the identity wrapper"
    );
    let tail = bytes.len() - (12 + 8 * locked.layer_count());
    assert_eq!(&bytes[tail..tail + 4], b"LIDS", "LIDS stays LAST");

    // The table this file assembles itself, from the flags alone.
    let mut table = b"LLNK".to_vec();
    table.extend(1u32.to_le_bytes());
    table.extend((locked.layer_count() as u32).to_le_bytes());
    for (locks, link) in [(LOCK_ALL, 0u32), (0, 1), (0, 1)] {
        table.extend(locks.to_le_bytes());
        table.extend(link.to_le_bytes());
    }
    let at = bytes
        .windows(4)
        .position(|w| w == b"LLNK")
        .expect("an LLNK table");
    assert_eq!(
        &bytes[at..at + table.len()],
        &table[..],
        "the table this file assembled is the one the writer emitted"
    );
    assert_eq!(at + table.len(), tail, "LLNK sits immediately before LIDS");

    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    let rows = info(&reopened);
    assert_eq!(rows[0]["locks"], json!(LOCK_ALL));
    assert_eq!(rows[1]["link"], json!(1));
    assert_eq!(rows[2]["link"], json!(1));
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes,
        "reopen then resave is byte-identical"
    );

    // Releasing the last lock and the last link restores the prior bytes.
    let cleared = must(&locked, json!({"op": "layer_lock", "layer": 0, "locks": 0}));
    let cleared = must(&cleared, json!({"op": "unlink_layers", "layers": [1, 2]}));
    assert_eq!(
        cleared.encode_native(LIMIT, 0, None).expect("cleared"),
        plain,
        "releasing the last lock restores the 12E-1 writer's exact bytes"
    );
}

#[test]
fn a_crafted_lock_table_refuses_the_open() {
    let base = nested();
    let locked = must(
        &base,
        json!({"op": "layer_lock", "layer": 0, "locks": LOCK_POSITION}),
    );
    let good = locked.encode_native(LIMIT, 0, None).expect("bytes");
    let at = good
        .windows(4)
        .position(|w| w == b"LLNK")
        .expect("an LLNK table");

    let refuse = |bytes: &[u8], what: &str| -> String {
        match PrecisionDocument::parse_native(bytes, LIMIT, 0, None) {
            Err(message) => message,
            Ok(_) => panic!("{what} must refuse the open"),
        }
    };

    let mut tag = good.clone();
    tag[at..at + 4].copy_from_slice(b"XXXX");
    assert!(refuse(&tag, "a bad tag").contains("layer lock schema"));

    let mut schema = good.clone();
    schema[at + 4..at + 8].copy_from_slice(&2u32.to_le_bytes());
    assert!(refuse(&schema, "a bad schema").contains("layer lock schema"));

    let mut count = good.clone();
    count[at + 8..at + 12].copy_from_slice(&2u32.to_le_bytes());
    assert!(refuse(&count, "a short count").contains("layer lock count"));

    // Reserved bits are MASKED, never refused: a file from a newer build
    // loads with the locks THIS build understands.
    let mut future = good.clone();
    future[at + 12..at + 16].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    let opened = PrecisionDocument::parse_native(&future, LIMIT, 0, None)
        .expect("reserved lock bits are masked, not refused");
    assert_eq!(info(&opened)[0]["locks"], json!(LOCK_ALL));
}

// ------------------------------------------------ inherited locks --

/// `Outside`, then group `G` holding `Child` and group `Inner`, which holds
/// `Grand`. Postorder: outside 0, child 1, grand 2, inner 3, G 4 — the byte
/// fixture's numbering in `lock_tests`.
fn chain() -> PrecisionDocument {
    let raster = |name: &str, v: u16| {
        Layer::raster(name, image(vec![[v, v, v, 65535]; 4]), (0, 0), 1.0, true).unwrap()
    };
    let inner = Layer::isolated_normal("Inner", vec![raster("Grand", 3000)], 1.0, true).unwrap();
    let g = Layer::isolated_normal("G", vec![raster("Child", 2000), inner], 1.0, true).unwrap();
    document(vec![raster("Outside", 1000), g])
}

const CHILD: usize = 1;
const GRAND: usize = 2;
const INNER: usize = 3;
const G: usize = 4;

fn lock(document: &PrecisionDocument, layer: usize, locks: u32) -> Box<PrecisionDocument> {
    must(
        document,
        json!({"op": "layer_lock", "layer": layer, "locks": locks}),
    )
}

fn typed_owner(document: &PrecisionDocument, index: usize, locks: u32) -> Option<usize> {
    let mut out = usize::MAX;
    unsafe { dr_precision_doc_lock_owner(document, index, locks, &mut out) }.then_some(out)
}

fn paint_request(layer: usize) -> Value {
    json!({"op": "paint", "layer": layer, "opacity": 1.0,
           "color": [1.0, 0.0, 0.0, 1.0], "color_space": "document"})
}

fn invert(document: &PrecisionDocument, layer: usize) -> Result<Option<PrecisionDocument>, String> {
    let description = json!({"type": "adjust", "op": "invert", "params": {},
                             "evaluation": "precision_v1"})
    .to_string();
    document.apply_adjustment(layer, &description, LIMIT, 0, None)
}

#[test]
fn a_group_s_locks_cover_its_contents_at_high_depth() {
    let base = chain();

    // Pixels on the group: paint and a filter refuse on a child and a
    // grandchild, and the sentence names the group.
    let pixels = lock(&base, G, LOCK_PIXELS);
    for index in [CHILD, GRAND] {
        assert_eq!(
            unsafe { dr_precision_doc_lock_block(&*pixels, index, KIND_PIXELS) },
            LOCK_PIXELS
        );
        let message = match edit_with(&pixels, paint_request(index), &coverage(65535)) {
            Err(message) => message,
            Ok(_) => panic!("paint on {index} under a pixel-locked group"),
        };
        assert!(
            message.contains(&format!(
                "high-depth layer {index} is inside group {G}, which is locked (pixels)"
            )),
            "{message}"
        );
        let message = match invert(&pixels, index) {
            Err(message) => message,
            Ok(_) => panic!("a filter on {index} under a pixel-locked group"),
        };
        assert!(message.contains("inside group 4"), "{message}");
        assert_eq!(typed_owner(&pixels, index, LOCK_PIXELS), Some(G));
    }
    // Outside the group, editing goes through.
    assert!(matches!(
        edit_with(&pixels, paint_request(0), &coverage(65535)),
        Ok(Some(_))
    ));
    assert_eq!(typed_owner(&pixels, 0, LOCK_PIXELS), None);
    // The nearest holder wins.
    let inner_too = lock(&pixels, INNER, LOCK_PIXELS);
    assert_eq!(typed_owner(&inner_too, GRAND, LOCK_PIXELS), Some(INNER));
    let grand_too = lock(&inner_too, GRAND, LOCK_PIXELS);
    assert_eq!(typed_owner(&grand_too, GRAND, LOCK_PIXELS), Some(GRAND));
    let message = match edit_with(&grand_too, paint_request(GRAND), &coverage(65535)) {
        Err(message) => message,
        Ok(_) => panic!("a pixel-locked record must refuse a paint"),
    };
    assert!(
        message.contains("high-depth layer 2 is locked (pixels)"),
        "its own lock names itself: {message}"
    );

    // Position on the group: moving a child refuses by the group's name.
    let position = lock(&base, G, LOCK_POSITION);
    let message = refusal(
        &position,
        json!({"op": "affine", "layer": CHILD, "target": "pixels",
               "matrix": [1.0, 0.0, 0.0, 1.0, 1.0, 0.0], "filter": "nearest"}),
    );
    assert!(
        message.contains("inside group 4, which is locked (position)"),
        "{message}"
    );

    // Lock All on the group freezes a child's mask; a UNION of bits across
    // the chain does not.
    let all = lock(&base, G, LOCK_ALL);
    let add = json!({"op": "set_mask", "layer": CHILD, "width": 2, "height": 2});
    let message = match edit_with(&all, add.clone(), &coverage(32768)) {
        Err(message) => message,
        Ok(_) => panic!("Lock All on the group must refuse a child's mask edit"),
    };
    assert!(
        message.contains("inside group 4, which is locked (all)"),
        "{message}"
    );
    let split = lock(&base, G, LOCK_PIXELS | LOCK_POSITION);
    let union = lock(&split, CHILD, LOCK_TRANSPARENCY);
    assert!(matches!(
        edit_with(&union, add, &coverage(32768)),
        Ok(Some(_))
    ));

    // Transparency on the group: a child's erase is the exact no-op.
    let transparency = lock(&base, G, LOCK_TRANSPARENCY);
    assert!(matches!(
        edit_with(
            &transparency,
            json!({"op": "erase", "layer": CHILD, "opacity": 1.0}),
            &coverage(65535)
        ),
        Ok(None)
    ));
    assert_eq!(
        typed_owner(&transparency, CHILD, LOCK_TRANSPARENCY),
        Some(G)
    );

    // Ungrouped, the child is out from under the lock.
    let ungrouped = must(&pixels, json!({"op": "ungroup_layer", "layer": G}));
    assert!(matches!(
        edit_with(&ungrouped, paint_request(CHILD), &coverage(65535)),
        Ok(Some(_))
    ));

    // The owner query's edges.
    assert_eq!(typed_owner(&base, CHILD, 0), None, "an empty mask");
    assert_eq!(typed_owner(&base, 99, LOCK_PIXELS), None, "out of range");
    assert!(!unsafe { dr_precision_doc_lock_owner(&base, CHILD, LOCK_ALL, ptr::null_mut()) });
}
