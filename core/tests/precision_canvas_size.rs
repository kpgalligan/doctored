//! Phase 12F-1: typed CANVAS SIZE at 16 bits and at 32-bit float.
//!
//! Two independent yardsticks, never one answer compared with itself:
//!
//! * every geometry expectation is WRITTEN OUT BY HAND here (each layer's new
//!   offset, each guide's new position, the padded channel sample by sample);
//! * the same numbers are ALSO produced by `DrDocument::canvas_resize` on a
//!   BYTE twin carrying the same offsets, masks, channel and chrome — the
//!   shipped implementation this op is specified against (G1). The channel
//!   comparison is the byte padded plane × 257, sample for sample.
//!
//! Exactness is checked the strongest way there is: every raster and mask
//! buffer is POINTER-equal before and after (`precision_resolution.rs`'s
//! shape), because a sample comparison would also pass on a copy.
//!
//! The float half: `canvas_size` MIGRATES (f32 buffers pointer-equal, v40
//! round trip identical), the integer-translation `affine` float
//! `move_layers` sends still re-points the buffer, and the FILTERED
//! `GeometryEdit`s — Image Size, a non-integer affine on pixels and on a mask,
//! a perspective, straighten — are ADMITTED at float (their oracle lives in
//! `core/tests/float_filtered_geometry.rs`). The EXACT canvas ops (zero-angle
//! crop, the quarter turns, the flips) live in
//! `core/tests/float_exact_geometry.rs`.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_guide::{Guide, GuideOrientation};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_doc::dr_doc_canvas_resize;
use doctored_core::ffi_precision::dr_precision_image_storage_identity;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 16;
const H: u32 = 12;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

const HORIZONTAL: GuideOrientation = GuideOrientation::Horizontal;
const VERTICAL: GuideOrientation = GuideOrientation::Vertical;

/// The five origins, each named by what it is: a centred grow, a shrink into
/// a corner, a positive origin, a negative one and an odd delta.
const CASES: &[(u32, u32, i32, i32, &str)] = &[
    (W + 8, H + 6, 4, 3, "a centred grow"),
    (
        W - 6,
        H - 4,
        -6,
        -4,
        "a shrink into the bottom-right corner",
    ),
    (W + 5, H + 3, 5, 3, "a positive origin"),
    (W - 4, H - 2, -3, -1, "a negative origin"),
    (W + 7, H + 3, 3, 1, "an odd delta"),
];

// ---------------------------------------------------------------- helpers --

fn parse(value: Value) -> GeometryEdit {
    GeometryEdit::parse(&value.to_string()).expect("a geometry request")
}

fn try_apply(doc: &PrecisionDocument, request: Value) -> Result<Option<PrecisionDocument>, String> {
    doc.geometry(&parse(request), LIMIT, 0, None)
}

fn apply(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    try_apply(doc, request)
        .expect("the edit runs")
        .expect("the edit changes something")
}

fn refusal(doc: &PrecisionDocument, request: Value) -> String {
    match try_apply(doc, request.clone()) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

fn canvas_size(width: u32, height: u32, x: i32, y: i32) -> Value {
    json!({"op": "canvas_size", "width": width, "height": height, "x": x, "y": y})
}

fn image(w: u32, h: u32, seed: u16) -> PrecisionImage {
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let v = seed.wrapping_add(i as u16);
            [v, v.wrapping_mul(3), 65535 - v, v | 0x8000]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).expect("an image")
}

fn plane_samples(w: u32, h: u32, seed: u32) -> Vec<u8> {
    (0..w * h).map(|i| ((i * 7 + seed) % 251) as u8).collect()
}

fn typed_mask(w: u32, h: u32, offset: (i32, i32), linked: bool, seed: u32) -> PrecisionMask {
    let mut properties = MaskProperties::at(offset);
    properties.linked = linked;
    let samples: Vec<u16> = plane_samples(w, h, seed)
        .into_iter()
        .map(|v| u16::from(v) * 257)
        .collect();
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, samples, None).expect("a mask plane"),
        properties,
        true,
    )
    .expect("a mask")
}

const CHANNEL_SEED: u32 = 11;
const GUIDES: [(GuideOrientation, f64); 4] = [
    (VERTICAL, 2.0),
    (VERTICAL, 13.5),
    (HORIZONTAL, 1.0),
    (HORIZONTAL, 10.25),
];
const ORIGIN: (f64, f64) = (3.0, 5.0);

/// The typed fixture: a canvas-sized base with a LINKED mask, a small raster
/// at a non-zero offset with an UNLINKED mask, a GROUP carrying a mask, and —
/// added afterwards, because it is an edit — an adjustment layer with a mask.
/// One saved channel, four guides and a moved ruler origin.
fn typed_fixture() -> PrecisionDocument {
    let inner = Layer::raster("Inner", image(6, 5, 9000), (2, 2), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Group", vec![inner], 1.0, true).unwrap();
    let base = Layer::raster("Base", image(W, H, 100), (0, 0), 1.0, true).unwrap();
    let small = Layer::raster("Small", image(7, 5, 30000), (5, 3), 1.0, true).unwrap();
    let channel = PrecisionChannel::new(
        "Coverage",
        PrecisionPlane::from_u16(
            W,
            H,
            plane_samples(W, H, CHANNEL_SEED)
                .into_iter()
                .map(|v| u16::from(v) * 257)
                .collect(),
            None,
        )
        .unwrap(),
    )
    .unwrap();
    let doc = PrecisionDocument::from_parts_and_channels(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution { x: 300.0, y: 150.0 },
        vec![base, small, group],
        vec![channel],
    )
    .expect("a typed document");
    // Postorder: 0 Base, 1 Small, 2 Inner, 3 Group.
    let doc = doc
        .set_layer_mask(0, Some(typed_mask(W, H, (2, -1), true, 1)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let doc = doc
        .set_layer_mask(1, Some(typed_mask(7, 5, (5, 3), false, 2)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let doc = doc
        .set_layer_mask(3, Some(typed_mask(W, H, (0, 0), true, 3)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let doc = doc
        .add_adjustment(
            "Tone",
            &json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"})
                .to_string(),
            1.0,
            LIMIT,
            0,
            None,
        )
        .expect("the adjustment is admitted")
        .expect("an insert is a change");
    let adjustment = (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == "Tone")
        .expect("the adjustment record");
    let doc = doc
        .set_layer_mask(
            adjustment,
            Some(typed_mask(W, H, (-2, 4), false, 4)),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let mut doc = doc;
    for (orientation, position) in GUIDES {
        doc = doc.add_guide(orientation, position).expect("a guide");
    }
    doc.set_ruler_origin(ORIGIN.0, ORIGIN.1)
        .expect("the origin moves")
}

/// The BYTE twin: the same offsets, the same mask placements, the same
/// channel plane and the same chrome. Pixel VALUES need not match — only the
/// geometry this test compares does.
fn byte_fixture() -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([10, 20, 30, 255])));
    doc = doc
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(7, 5, Rgba([40, 50, 60, 255])),
            "Small",
        )
        .expect("a second layer");
    doc.layers[1].offset = (5, 3);
    doc.layers[0].mask = Some(Arc::new(
        GrayImage::from_raw(W, H, plane_samples(W, H, 1)).unwrap(),
    ));
    doc.layers[0].mask_properties = {
        let mut p = MaskProperties::at((2, -1));
        p.linked = true;
        p
    };
    doc.layers[1].mask = Some(Arc::new(
        GrayImage::from_raw(7, 5, plane_samples(7, 5, 2)).unwrap(),
    ));
    doc.layers[1].mask_properties = MaskProperties::at((5, 3));
    let mut doc = doc
        .add_channel(
            "Coverage",
            &plane_samples(W, H, CHANNEL_SEED),
            W,
            H,
            [255, 0, 0],
            0.5,
        )
        .expect("a channel");
    for (orientation, position) in GUIDES {
        doc = doc.add_guide(orientation, position).expect("a guide");
    }
    doc.set_ruler_origin(ORIGIN.0, ORIGIN.1)
        .expect("the origin moves")
}

/// Each raster record's STORAGE IDENTITY, in postorder.
fn raster_addresses(doc: &PrecisionDocument) -> Vec<usize> {
    (0..doc.layer_count())
        .filter_map(|i| doc.layer_source(i))
        .map(|image| unsafe { dr_precision_image_storage_identity(image) as usize })
        .collect()
}

/// Each mask plane's sample-buffer ADDRESS, in postorder.
fn mask_addresses(doc: &PrecisionDocument) -> Vec<usize> {
    (0..doc.layer_count())
        .filter_map(|i| doc.layer(i).unwrap().mask())
        .map(|m| m.plane().samples().as_ptr() as usize)
        .collect()
}

/// Every record's `(name, is raster, offset, mask offset)`, in postorder —
/// the whole placement of the document in one comparable value. Only a RASTER
/// record has an offset of its own to move (`canvas_resize`'s own
/// `LayerKind::Raster` guard); a group or an adjustment has only its mask.
type Placement = (String, bool, (i32, i32), Option<(i32, i32)>);

fn placement(doc: &PrecisionDocument) -> Vec<Placement> {
    (0..doc.layer_count())
        .map(|i| {
            let layer = doc.layer(i).unwrap();
            (
                layer.name().to_string(),
                doc.layer_source(i).is_some(),
                layer.offset(),
                layer.mask().map(|m| m.properties().offset),
            )
        })
        .collect()
}

fn pairs(guides: &[Guide]) -> Vec<(GuideOrientation, f64)> {
    guides.iter().map(|g| (g.orientation, g.position)).collect()
}

// ----------------------------------- T1: the byte twin and the hand table --

#[test]
fn canvas_size_moves_every_plane_by_the_origin_and_agrees_with_the_byte_twin() {
    let typed = typed_fixture();
    let byte = byte_fixture();
    let before = placement(&typed);

    for &(width, height, x, y, what) in CASES {
        let out = apply(&typed, canvas_size(width, height, x, y));
        assert_eq!(out.dimensions(), (width, height), "{what}: the canvas");
        assert_eq!(
            out.resolution(),
            typed.resolution(),
            "{what}: Canvas Size never transposes the PPI"
        );

        // HAND-WRITTEN: every record's offset, and every mask's offset,
        // moved by exactly (x, y) and by nothing else.
        let expected: Vec<Placement> = before
            .iter()
            .map(|(name, raster, offset, mask)| {
                (
                    name.clone(),
                    *raster,
                    if *raster {
                        (offset.0 + x, offset.1 + y)
                    } else {
                        *offset
                    },
                    mask.map(|m| (m.0 + x, m.1 + y)),
                )
            })
            .collect();
        assert_eq!(placement(&out), expected, "{what}: every placement moves");

        // THE BYTE TWIN answers the same numbers for the records it shares.
        let twin = byte
            .canvas_resize(width, height, (x, y))
            .expect("the byte canvas resize");
        assert_eq!(
            (twin.width, twin.height),
            out.dimensions(),
            "{what}: the same canvas"
        );
        for (index, name) in ["Base", "Small"].iter().enumerate() {
            let typed_row = placement(&out)
                .into_iter()
                .find(|(n, ..)| n == name)
                .expect("the record");
            assert_eq!(
                typed_row.2, twin.layers[index].offset,
                "{what}: {name}'s offset equals the byte answer"
            );
            assert_eq!(
                typed_row.3,
                Some(twin.layers[index].mask_properties.offset),
                "{what}: {name}'s mask offset equals the byte answer"
            );
        }

        // The CHANNEL is the byte padded plane × 257, sample for sample.
        let samples = out.channels()[0].plane().samples();
        let reference = twin.channels[0].data.as_raw();
        assert_eq!(
            out.channels()[0].plane().dimensions(),
            (width, height),
            "{what}: the channel is canvas-sized"
        );
        assert_eq!(samples.len(), reference.len(), "{what}: channel length");
        for (i, (&got, &want)) in samples.iter().zip(reference).enumerate() {
            assert_eq!(
                got,
                u16::from(want) * 257,
                "{what}: channel sample {i} equals the byte padded plane × 257"
            );
        }
    }
}

// ------------------------------------------- T2: allocations and the kinds --

#[test]
fn every_raster_and_mask_keeps_its_allocation_and_every_mask_kind_moves() {
    let typed = typed_fixture();
    let rasters = raster_addresses(&typed);
    let masks = mask_addresses(&typed);
    assert_eq!(rasters.len(), 3, "Base, Small and the grouped Inner");
    assert_eq!(
        masks.len(),
        4,
        "a linked raster mask, an unlinked one, a GROUP mask and an ADJUSTMENT mask"
    );
    // The four mask kinds are the ones the claim names.
    let kinds: Vec<(String, bool)> = (0..typed.layer_count())
        .filter_map(|i| {
            let layer = typed.layer(i).unwrap();
            layer
                .mask()
                .map(|m| (layer.name().to_string(), m.properties().linked))
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("Base".into(), true),
            ("Small".into(), false),
            ("Group".into(), true),
            ("Tone".into(), false),
        ]
    );

    let out = apply(&typed, canvas_size(W + 8, H + 6, 4, 3));
    assert_eq!(
        raster_addresses(&out),
        rasters,
        "a Canvas Size reads no pixel: every raster buffer is SHARED"
    );
    assert_eq!(
        mask_addresses(&out),
        masks,
        "and so is every mask plane, linked or not, on a raster, a group or an adjustment"
    );
    // Every mask MOVED, all four of them.
    for i in 0..out.layer_count() {
        let (before, after) = (
            typed
                .layer(i)
                .unwrap()
                .mask()
                .map(|m| m.properties().offset),
            out.layer(i).unwrap().mask().map(|m| m.properties().offset),
        );
        if let (Some(before), Some(after)) = (before, after) {
            assert_eq!(
                after,
                (before.0 + 4, before.1 + 3),
                "the mask on {} rode the canvas",
                out.layer(i).unwrap().name()
            );
        }
    }
}

// ------------------------------------------------------------ T3: chrome --

#[test]
fn the_chrome_is_padded_guides_and_padded_origin_shifted_dropped_and_clamped() {
    let typed = typed_fixture();
    let byte = byte_fixture();

    // A GROW: every guide survives, shifted by the origin; the origin moves
    // with it and stays inside.
    let grown = apply(&typed, canvas_size(W + 8, H + 6, 4, 3));
    assert_eq!(
        pairs(grown.guides()),
        vec![
            (HORIZONTAL, 4.0),
            (HORIZONTAL, 13.25),
            (VERTICAL, 6.0),
            (VERTICAL, 17.5),
        ],
        "hand-written: every guide shifted by (4, 3)"
    );
    assert_eq!(grown.ruler_origin(), (ORIGIN.0 + 4.0, ORIGIN.1 + 3.0));
    let twin = byte.canvas_resize(W + 8, H + 6, (4, 3)).expect("byte grow");
    assert_eq!(
        pairs(grown.guides()),
        pairs(&twin.guides),
        "and the byte twin says the same"
    );
    assert_eq!(grown.ruler_origin(), twin.ruler_origin);

    // A SHRINK into the corner: guides that left the window are DROPPED, and
    // the ruler origin is CLAMPED rather than dropped.
    let shrunk = apply(&typed, canvas_size(W - 6, H - 4, -6, -4));
    assert_eq!(
        pairs(shrunk.guides()),
        vec![(HORIZONTAL, 6.25), (VERTICAL, 7.5)],
        "hand-written: (HORIZONTAL, 1.0) and (VERTICAL, 2.0) left the window"
    );
    assert_eq!(
        shrunk.ruler_origin(),
        (0.0, 1.0),
        "hand-written: (3 - 6, 5 - 4) clamped into a 10 x 8 canvas"
    );
    let twin = byte
        .canvas_resize(W - 6, H - 4, (-6, -4))
        .expect("byte shrink");
    assert_eq!(pairs(shrunk.guides()), pairs(&twin.guides));
    assert_eq!(shrunk.ruler_origin(), twin.ruler_origin);
}

// ------------------------------------------- T4: the no-op and the refusals --

#[test]
fn the_exact_no_op_answers_none_and_each_refusal_states_its_own_sentence() {
    let typed = typed_fixture();

    assert!(
        try_apply(&typed, canvas_size(W, H, 0, 0))
            .expect("an identity is not an error")
            .is_none(),
        "the same size at origin (0, 0) changes nothing, so the typed purity rule answers None"
    );
    // Phase 12F-1b: the BYTE twin says the same now. An identical copy would
    // register a phantom undo step, so `canvas_resize` answers `None` for the
    // exact no-op and `dr_doc_canvas_resize` is NULL for it.
    let byte = byte_fixture();
    assert!(
        byte.canvas_resize(W, H, (0, 0)).is_none(),
        "the byte op answers None for the exact no-op too"
    );
    assert!(
        unsafe { dr_doc_canvas_resize(&byte, W, H, 0, 0) }.is_null(),
        "and the export is NULL for it"
    );
    // Anything that WOULD change still comes back.
    assert!(byte.canvas_resize(W, H, (1, 0)).is_some());
    assert!(byte.canvas_resize(W + 1, H, (0, 0)).is_some());
    // The CHROME is folded into the result rather than carried verbatim, and
    // counts toward `changed` — a shrink that drops a guide proves the fold
    // (a chrome-ONLY Canvas Size cannot exist: the origin and extent that
    // move a guide move every layer too).
    let shrunk = apply(&typed, canvas_size(W - 6, H - 4, -6, -4));
    assert_eq!(shrunk.guides().len(), 2, "the chrome was recomputed");

    // ZERO size and a canvas past MAX_PIXELS answer the pixel-limit sentence.
    for (width, height, what) in [(0, H, "a zero width"), (W, 0, "a zero height")] {
        assert!(
            refusal(&typed, canvas_size(width, height, 0, 0))
                .contains("dimensions exceed the supported pixel limit"),
            "{what}"
        );
    }
    assert!(
        refusal(&typed, canvas_size(40000, 40000, 0, 0))
            .contains("dimensions exceed the supported pixel limit"),
        "1.6 gigapixels is past MAX_PIXELS"
    );

    // An offset that would leave i32 REFUSES, where the byte op saturates.
    assert_eq!(
        refusal(&typed, canvas_size(W, H, i32::MAX, 0)),
        "typed geometry offset exceeds i32 coordinates"
    );
    assert_eq!(
        refusal(&typed, canvas_size(W, H, 0, i32::MIN)),
        "typed geometry offset exceeds i32 coordinates"
    );

    // CHANNEL CAPACITY: ten canvas-sized channels do not fit a 100 MP canvas.
    let mut many = typed_fixture();
    for i in 0..9 {
        let plane = PrecisionPlane::from_u16(W, H, vec![(i * 1000) as u16; (W * H) as usize], None)
            .unwrap();
        let channel = PrecisionChannel::new(&format!("Extra {i}"), plane).unwrap();
        let mut channels = many.channels().to_vec();
        channels.push(channel);
        many = PrecisionDocument::from_parts_and_channels(
            many.dimensions().0,
            many.dimensions().1,
            many.profile().clone(),
            Metadata::default(),
            many.resolution(),
            many.root_layers().to_vec(),
            channels,
        )
        .expect("a document with more channels");
    }
    assert_eq!(many.channels().len(), 10);
    assert_eq!(
        refusal(&many, canvas_size(10_000, 10_000, 0, 0)),
        "At 16 bits the result is too large to save and reopen on its own, so closing \
         other documents will not help. Use a smaller image or fewer layers, masks or \
         channels."
    );
}

#[test]
fn a_locked_layer_does_not_refuse_and_an_unknown_key_does() {
    // Canvas Size is lock-EXEMPT, like every other whole-canvas op: it moves
    // the document, not a layer.
    let typed = typed_fixture();
    let request =
        CString::new(json!({"op": "layer_lock", "layer": 0, "locks": 0xF}).to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &typed,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "Lock All is applied");
    let locked = unsafe { Box::from_raw(out) };
    assert!(try_apply(&locked, canvas_size(W + 4, H + 4, 2, 2))
        .expect("a locked layer does not refuse a canvas op")
        .is_some());

    // An unknown key refuses: `GeometryEdit` is `deny_unknown_fields`.
    let message = GeometryEdit::parse(
        &json!({"op": "canvas_size", "width": W, "height": H, "x": 0, "y": 0, "z": 1}).to_string(),
    )
    .expect_err("an unknown key must refuse");
    assert!(
        message.contains("invalid typed geometry request"),
        "{message}"
    );
}

// -------------------------------------------------------- T5: persistence --

#[test]
fn native_save_reopen_and_resave_is_byte_identical_with_the_new_canvas() {
    let typed = typed_fixture();
    let out = apply(&typed, canvas_size(W + 8, H + 6, 4, 3));
    let bytes = out.encode_native(LIMIT, 0, None).expect("native bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");

    assert_eq!(reopened.dimensions(), (W + 8, H + 6));
    assert_eq!(placement(&reopened), placement(&out), "offsets survive");
    assert_eq!(
        reopened.channels()[0].plane().samples(),
        out.channels()[0].plane().samples(),
        "the padded channel survives"
    );
    assert_eq!(
        pairs(reopened.guides()),
        pairs(out.guides()),
        "the LGDE chrome survives"
    );
    assert_eq!(reopened.ruler_origin(), out.ruler_origin());
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("resave"),
        bytes,
        "reopen then resave is byte-identical"
    );
}

#[test]
fn the_capability_table_admits_geometry_canvas_size_at_both_typed_depths() {
    for depth in [16u32, 32] {
        let answer = capability("geometry_canvas_size", "document", depth);
        assert_eq!(
            answer["supported"], true,
            "geometry_canvas_size targets the document at depth {depth}: {answer}"
        );
    }
    let answer = capability("geometry_canvas_size", "raster", 16);
    assert_eq!(
        answer["supported"], false,
        "its only target is the document: {answer}"
    );
}

// ------------------------------------------------------------ T6: float --

fn float_fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    Some(PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).expect("the fixture"))
}

/// The operator table a host reads ADMITS the geometry families and carries
/// no geometry refusal row.
fn assert_float_geometry_admitted() {
    let table: Value =
        serde_json::from_str(&doctored_core::float_document::float_operators_json()).unwrap();
    assert!(
        table["refused"].as_array().unwrap().iter().all(|row| {
            !row["operator"]
                .as_str()
                .is_some_and(|op| op.contains("Image Size") || op.contains("free transform"))
        }),
        "no refused row names the filtered geometry: {table}"
    );
    let admitted = table["admitted"].as_array().unwrap();
    for family in [
        "geometry_canvas_size",
        "geometry_crop",
        "geometry_rotate",
        "geometry_flip",
        "geometry_resize",
        "geometry_straighten",
        "geometry_rotate_canvas",
        "geometry_affine",
        "geometry_perspective",
        "geometry_affine_set",
        "geometry_perspective_set",
        "mask_transform",
    ] {
        assert!(
            admitted.iter().any(|v| v == family),
            "{family} is in the admitted set"
        );
    }
}

#[test]
fn canvas_size_migrates_to_float_with_shared_buffers_and_an_identical_round_trip() {
    let Some(base) = float_fixture() else {
        return;
    };
    assert!(base.is_float());
    let (w, h) = base.dimensions();
    let before = raster_addresses(&base);

    // The `image_equal` trap: origin (0, 0) makes the planned location equal
    // the layer's own offset, which is the ONLY way that comparison was ever
    // reached. A `Keep` plane short-circuits it.
    let grown = apply(&base, canvas_size(w + 10, h + 6, 0, 0));
    assert_eq!(grown.dimensions(), (w + 10, h + 6));
    assert_eq!(
        raster_addresses(&grown),
        before,
        "every f32 buffer is SHARED at origin (0, 0)"
    );
    for i in 0..grown.layer_count() {
        assert_eq!(
            grown.layer(i).unwrap().offset(),
            base.layer(i).unwrap().offset(),
            "origin (0, 0) moves nothing"
        );
    }

    let shifted = apply(&base, canvas_size(w + 10, h + 6, 5, 4));
    assert_eq!(
        raster_addresses(&shifted),
        before,
        "and shared under a SHIFTED grow too"
    );
    for i in 0..shifted.layer_count() {
        let (bx, by) = base.layer(i).unwrap().offset();
        assert_eq!(shifted.layer(i).unwrap().offset(), (bx + 5, by + 4));
    }

    let bytes = shifted.encode_native(LIMIT, 0, None).expect("v40 bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert!(reopened.is_float());
    assert_eq!(reopened.dimensions(), shifted.dimensions());
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("resave"),
        bytes,
        "the v40 round trip is identical"
    );

    // An identity is still an identity at float.
    assert!(try_apply(&base, canvas_size(w, h, 0, 0))
        .expect("an identity is not an error")
        .is_none());
}

#[test]
fn the_filtered_typed_geometry_is_admitted_at_float() {
    let Some(base) = float_fixture() else {
        return;
    };
    assert_float_geometry_admitted();
    let (w, h) = base.dimensions();
    // A mask, so the mask target has a plane to resample (`mask_set` IS
    // admitted at float).
    let base = base
        .set_layer_mask(0, Some(typed_mask(w, h, (0, 0), true, 5)), LIMIT, 0, None)
        .expect("a float mask is admitted")
        .expect("attaching a mask is a change");

    let requests = vec![
        json!({"op": "resize", "width": w / 2, "height": h / 2, "filter": "bilinear"}),
        // A non-integer affine, on pixels and on a mask.
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": [1.0, 0.0, 0.0, 1.0, 0.25, 0.5], "filter": "bilinear"}),
        json!({"op": "affine", "layer": 0, "target": "mask",
               "matrix": [1.0, 0.0, 0.0, 1.0, 0.25, 0.5], "filter": "bilinear"}),
        json!({"op": "perspective", "layer": 0, "target": "pixels",
               "quad": [0.0, 0.0, f64::from(w), 1.0, f64::from(w), f64::from(h),
                        0.0, f64::from(h)], "filter": "bilinear"}),
        json!({"op": "straighten", "matrix": [0.99, 0.13, -0.13, 0.99, 1.0, -1.0],
               "filter": "bilinear", "x": 1, "y": 1, "width": w - 2, "height": h - 2}),
    ];
    for request in requests {
        let edit = parse(request.clone());
        base.geometry_plan(&edit)
            .unwrap_or_else(|e| panic!("geometry_plan admits {request}: {e}"));
        let out = base
            .geometry(&edit, LIMIT, 0, None)
            .unwrap_or_else(|e| panic!("geometry admits {request}: {e}"))
            .unwrap_or_else(|| panic!("{request} changes something"));
        assert!(out.is_float(), "{request} keeps the float mode");
        assert_eq!(out.float_refusal(), None, "{request} leaves no refusal");
    }
}

#[test]
fn the_integer_translation_float_move_layers_sends_is_still_admitted() {
    let Some(base) = float_fixture() else {
        return;
    };
    let before = raster_addresses(&base);
    let moved = apply(
        &base,
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": [1.0, 0.0, 0.0, 1.0, 7.0, -3.0], "filter": "nearest"}),
    );
    assert_eq!(
        raster_addresses(&moved),
        before,
        "an integer translation re-points the buffer and never copies it"
    );
    let (bx, by) = base.layer(0).unwrap().offset();
    assert_eq!(moved.layer(0).unwrap().offset(), (bx + 7, by - 3));

    // The ZERO translation answers `None` rather than the RGBA16 error the
    // accidental `image_equal` path used to raise.
    assert!(
        try_apply(
            &base,
            json!({"op": "affine", "layer": 0, "target": "pixels",
                   "matrix": [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], "filter": "nearest"}),
        )
        .expect("a zero translation is not an error")
        .is_none(),
        "the identity is an exact no-op, not a storage refusal"
    );
}
