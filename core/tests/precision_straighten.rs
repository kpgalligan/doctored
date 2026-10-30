//! Phase 12F-1b2: typed STRAIGHTEN — `GeometryEdit::Straighten`, ONE op equal
//! to the byte composition `straighten_layers(m)` + `transform_channels(m)` +
//! `crop(x, y, w, h)`.
//!
//! Two independent yardsticks, never one answer compared with itself:
//!
//! * the FILTERED cases are compared against an f64 inverse-mapping resampler
//!   — pixel-centre convention, transparent out-of-bounds taps, premultiplied
//!   accumulation, normalized weights — and against hand-written destination
//!   extents (`bounds`, the reference's own snap/floor/ceil of the four
//!   transformed corners). Both live in `common/inverse_mapping.rs` since
//!   Phase 12F-1b3, where the arbitrary canvas rotation's own test file
//!   measures against the SAME reference at a maximum code of 255; the `u16`
//!   wrappers below are what keep every test body in this file unchanged;
//! * the EXACT cases are compared against the shipped ops they must equal
//!   (`crop`, `rotate90` + `crop`) bit for bit, with the raster and mask
//!   buffers POINTER-equal where the plan is `Keep`, and the placements are
//!   ALSO compared against the BYTE twin's own three-call chain, which is the
//!   specification.
//!
//! TOLERANCES (G7, fixed before measurement, never widened):
//!
//! * exact cases: bit-for-bit, buffers pointer-equal where `Keep`;
//! * filtered vs this file's f64 reference: alpha ≤ 1 code of 65535
//!   EVERYWHERE; straight RGB ≤ 1 code where the reference alpha code ≥ 1;
//!   stored RGBA == 0 where the result's own alpha rounds to 0 — a
//!   transparent store carries no colour to compare, which is that clause's
//!   whole point and is what proves the LOUD hidden RGB at alpha 0 never
//!   bleeds (premultiplied accumulation weights it by zero);
//! * scalar planes (masks, channels): ≤ 1 code of 65535 everywhere.
//!
//! Test (c)'s exact 90° matrix is CORE-ONLY: no host path can send it (the
//! Crop tool clamps the angle to −45…45 and the `crop` MCP tool refuses
//! `abs(angle) > 45`), so it has no drive or probe twin and is asserted here.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_guide::{Guide, GuideOrientation};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::doc_transform::Affine;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use doctored_core::style::{Effect, EffectKind, LayerStyle};
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::inverse_mapping;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The maximum code of the u16 storage every case here measures.
const MAX_CODE: u32 = 65535;
const W: u32 = 24;
const H: u32 = 18;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

const HORIZONTAL: GuideOrientation = GuideOrientation::Horizontal;
const VERTICAL: GuideOrientation = GuideOrientation::Vertical;

/// The straighten window every filtered case uses. It TOUCHES the rotated
/// content's bounding box on the left and top (the `Small` layer at (5, 3)
/// turned about this window's own centre reaches past both edges), so the
/// comparison covers destination pixels whose taps fall partly outside.
const WINDOW: (u32, u32, u32, u32) = (3, 2, 18, 13);
/// The straighten angle, in degrees, and the sign convention the host uses:
/// the Crop tool rotates by `−angle` about the box's centre.
const ANGLE: f64 = 7.5;

const CHANNEL_SEED: u32 = 11;
const GUIDES: [(GuideOrientation, f64); 4] = [
    (VERTICAL, 2.0),
    (VERTICAL, 19.5),
    (HORIZONTAL, 1.0),
    (HORIZONTAL, 8.25),
];
const ORIGIN: (f64, f64) = (4.0, 6.0);

// ------------------------------------------------------- request helpers --

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

fn straighten(matrix: [f64; 6], filter: &str, window: (u32, u32, u32, u32)) -> Value {
    json!({"op": "straighten", "matrix": matrix, "filter": filter,
           "x": window.0, "y": window.1, "width": window.2, "height": window.3})
}

/// The identical matrix `EditorViewController+Crop.commitCropSession` and
/// `AgentServer+Retouch.crop` build: rotate by `−angle` about the window's
/// centre, in canvas coordinates.
fn rotation(angle: f64, window: (u32, u32, u32, u32)) -> [f64; 6] {
    let cx = f64::from(window.0) + f64::from(window.2) / 2.0;
    let cy = f64::from(window.1) + f64::from(window.3) / 2.0;
    let radians = -angle * std::f64::consts::PI / 180.0;
    let (s, c) = (radians.sin(), radians.cos());
    // T(cx, cy) . R(radians) . T(-cx, -cy)
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

// ------------------------------------------------- this file's own oracle --

/// `m` with the window shift folded in: `T(−x, −y) ∘ m`.
fn windowed(m: [f64; 6], x: u32, y: u32) -> [f64; 6] {
    [
        m[0],
        m[1],
        m[2],
        m[3],
        m[4] - f64::from(x),
        m[5] - f64::from(y),
    ]
}

fn invert(m: [f64; 6]) -> [f64; 6] {
    inverse_mapping::invert(m)
}

/// The destination extent of a plane at `offset` with `dims` under `m`.
fn bounds(offset: (i32, i32), dims: (u32, u32), m: [f64; 6]) -> (i32, i32, u32, u32) {
    inverse_mapping::bounds(offset, dims, m)
}

fn widen_rgba(samples: &[[u16; 4]]) -> Vec<[u32; 4]> {
    samples
        .iter()
        .map(|p| {
            [
                u32::from(p[0]),
                u32::from(p[1]),
                u32::from(p[2]),
                u32::from(p[3]),
            ]
        })
        .collect()
}

fn narrow_rgba(samples: Vec<[u32; 4]>) -> Vec<[u16; 4]> {
    samples
        .into_iter()
        .map(|p| [p[0] as u16, p[1] as u16, p[2] as u16, p[3] as u16])
        .collect()
}

/// The shared f64 RGBA reference at this file's maximum code
/// (`common/inverse_mapping.rs`), in u16 so no test body changed when it
/// moved out.
fn oracle_rgba(
    source: &[[u16; 4]],
    dims: (u32, u32),
    source_offset: (i32, i32),
    dest: (i32, i32, u32, u32),
    inverse: [f64; 6],
    kind: &str,
) -> Vec<[u16; 4]> {
    narrow_rgba(inverse_mapping::reference_rgba(
        &widen_rgba(source),
        dims,
        source_offset,
        dest,
        inverse,
        kind,
        MAX_CODE,
    ))
}

/// The scalar twin, with the plane's own `outside` coverage.
fn oracle_plane(
    source: &[u16],
    dims: (u32, u32),
    source_offset: (i32, i32),
    dest: (i32, i32, u32, u32),
    inverse: [f64; 6],
    kind: &str,
    outside: u16,
) -> Vec<u16> {
    let widened: Vec<u32> = source.iter().map(|&v| u32::from(v)).collect();
    inverse_mapping::reference_plane(
        &widened,
        dims,
        source_offset,
        dest,
        inverse,
        kind,
        u32::from(outside),
        MAX_CODE,
    )
    .into_iter()
    .map(|v| v as u16)
    .collect()
}

// ------------------------------------------------------- G7's comparisons --

fn compare_rgba(got: &[[u16; 4]], want: &[[u16; 4]], what: &str) {
    inverse_mapping::assert_rgba_within_one(&widen_rgba(got), &widen_rgba(want), what);
}

fn compare_plane(got: &[u16], want: &[u16], what: &str) {
    let widen = |s: &[u16]| s.iter().map(|&v| u32::from(v)).collect::<Vec<u32>>();
    inverse_mapping::assert_plane_within_one(&widen(got), &widen(want), what);
}

// ------------------------------------------------------------- fixtures --

fn image(w: u32, h: u32, seed: u16) -> PrecisionImage {
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let v = seed.wrapping_add((i * 37) as u16);
            [v, v.wrapping_mul(3), 65535 - v, 65535]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).expect("an image")
}

/// The layer test (d) is about: a translucent one-pixel ring inside a fully
/// TRANSPARENT border whose stored RGB is LOUD magenta. Premultiplied
/// accumulation must weight that colour by zero, so it can never bleed into a
/// neighbouring sample.
fn edge_image(w: u32, h: u32) -> PrecisionImage {
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let v = (7000u16).wrapping_add((i * 911) as u16);
            if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                [65535, 0, 65535, 0]
            } else if x == 1 || y == 1 || x == w - 2 || y == h - 2 {
                [v, v.wrapping_mul(5), 65535 - v, 32896]
            } else {
                [v, v.wrapping_mul(3), 65535 - v, 65535]
            }
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).expect("an image")
}

fn plane_samples(w: u32, h: u32, seed: u32) -> Vec<u8> {
    (0..w * h).map(|i| ((i * 7 + seed) % 251) as u8).collect()
}

fn typed_mask(
    w: u32,
    h: u32,
    offset: (i32, i32),
    linked: bool,
    seed: u32,
    outside: u8,
) -> PrecisionMask {
    let mut properties = MaskProperties::at(offset);
    properties.linked = linked;
    properties.outside_coverage = outside;
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

const SMALL_AT: (i32, i32) = (5, 3);
const SMALL_DIMS: (u32, u32) = (9, 7);

/// The typed fixture: a canvas-sized base with a LINKED mask, the translucent
/// `Small` raster at a non-zero offset with an UNLINKED mask, a GROUP with a
/// mask, and an adjustment layer with an unlinked mask whose OUTSIDE coverage
/// is 255. One saved channel, four guides, a moved ruler origin.
fn typed_fixture() -> PrecisionDocument {
    let inner = Layer::raster("Inner", image(6, 5, 9000), (2, 2), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Group", vec![inner], 1.0, true).unwrap();
    let base = Layer::raster("Base", image(W, H, 100), (0, 0), 1.0, true).unwrap();
    let small = Layer::raster(
        "Small",
        edge_image(SMALL_DIMS.0, SMALL_DIMS.1),
        SMALL_AT,
        1.0,
        true,
    )
    .unwrap();
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
        .set_layer_mask(
            0,
            Some(typed_mask(W, H, (2, -1), true, 1, 0)),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let doc = doc
        .set_layer_mask(
            1,
            Some(typed_mask(
                SMALL_DIMS.0,
                SMALL_DIMS.1,
                SMALL_AT,
                false,
                2,
                0,
            )),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let doc = doc
        .set_layer_mask(
            3,
            Some(typed_mask(W, H, (0, 0), true, 3, 0)),
            LIMIT,
            0,
            None,
        )
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
            // OUTSIDE 255: the one mask whose untouched region is opaque.
            Some(typed_mask(W, H, (-2, 4), false, 4, 255)),
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

/// The BYTE twin: the two raster records the typed fixture shares with it, at
/// the same offsets with the same mask placements, plus the same channel and
/// the same chrome. Pixel VALUES need not match — only the geometry compared
/// here does.
fn byte_fixture() -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([10, 20, 30, 255])));
    doc = doc
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(SMALL_DIMS.0, SMALL_DIMS.1, Rgba([40, 50, 60, 200])),
            "Small",
        )
        .expect("a second layer");
    doc.layers[1].offset = SMALL_AT;
    doc.layers[0].mask = Some(Arc::new(
        GrayImage::from_raw(W, H, plane_samples(W, H, 1)).unwrap(),
    ));
    doc.layers[0].mask_properties = {
        let mut p = MaskProperties::at((2, -1));
        p.linked = true;
        p
    };
    doc.layers[1].mask = Some(Arc::new(
        GrayImage::from_raw(SMALL_DIMS.0, SMALL_DIMS.1, plane_samples(9, 7, 2)).unwrap(),
    ));
    doc.layers[1].mask_properties = MaskProperties::at(SMALL_AT);
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

/// The byte three-call chain this op is specified against.
fn byte_chain(
    doc: &DrDocument,
    matrix: [f64; 6],
    filter: image::imageops::FilterType,
    window: (u32, u32, u32, u32),
) -> DrDocument {
    let m = Affine::from_array(matrix);
    let straightened = doc
        .straighten_layers(m, filter)
        .expect("the byte straighten answers");
    let with_channels = straightened
        .transform_channels(m, filter)
        .unwrap_or(straightened);
    with_channels
        .crop(window.0, window.1, window.2, window.3)
        .expect("the byte crop answers")
}

/// Every raster record's u16 image, in postorder (compared by IDENTITY).
fn rasters(doc: &PrecisionDocument) -> Vec<PrecisionImage> {
    (0..doc.layer_count())
        .filter_map(|i| doc.layer_source(i))
        .inspect(|image| {
            assert_eq!(
                image.storage(),
                PrecisionStorage::Rgba16Unorm,
                "a 16-bit document's rasters are RGBA16"
            )
        })
        .cloned()
        .collect()
}

/// The same raster IMAGES, pairwise by identity (`PrecisionImage::ptr_eq`).
fn same_rasters(a: &[PrecisionImage], b: &[PrecisionImage]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| PrecisionImage::ptr_eq(x, y))
}

fn mask_addresses(doc: &PrecisionDocument) -> Vec<usize> {
    (0..doc.layer_count())
        .filter_map(|i| doc.layer(i).unwrap().mask())
        .map(|m| m.plane().samples().as_ptr() as usize)
        .collect()
}

type Placement = (
    String,
    Option<(i32, i32, u32, u32)>,
    Option<(i32, i32, u32, u32)>,
);

/// Every record's `(name, raster extent, mask extent)` in postorder — the
/// whole placement of the document in one comparable value.
fn placement(doc: &PrecisionDocument) -> Vec<Placement> {
    (0..doc.layer_count())
        .map(|i| {
            let layer = doc.layer(i).unwrap();
            let raster = doc.layer_source(i).map(|image| {
                let (w, h) = image.dimensions();
                (layer.offset().0, layer.offset().1, w, h)
            });
            let mask = layer.mask().map(|m| {
                let (w, h) = m.plane().dimensions();
                (m.properties().offset.0, m.properties().offset.1, w, h)
            });
            (layer.name().to_string(), raster, mask)
        })
        .collect()
}

fn named<'a>(doc: &'a PrecisionDocument, name: &str) -> &'a Layer {
    (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap())
        .find(|l| l.name() == name)
        .unwrap_or_else(|| panic!("no record named {name}"))
}

fn samples_of(doc: &PrecisionDocument, name: &str) -> Vec<[u16; 4]> {
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .and_then(|i| doc.layer_source(i))
        .expect("a raster record")
        .to_rgba16()
        .expect("u16 storage")
}

fn pairs(guides: &[Guide]) -> Vec<(GuideOrientation, f64)> {
    guides.iter().map(|g| (g.orientation, g.position)).collect()
}

fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if out.is_null() {
        let message = if error.is_null() {
            "no error".to_string()
        } else {
            unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned()
        };
        panic!("{request} refused: {message}");
    }
    *unsafe { Box::from_raw(out) }
}

// ----------------------------------------- (a) identity + window == crop --

#[test]
fn an_identity_matrix_over_a_window_is_the_crop_bit_for_bit() {
    let typed = typed_fixture();
    let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let (x, y, w, h) = WINDOW;

    let cropped = apply(
        &typed,
        json!({"op": "crop", "x": x, "y": y, "width": w, "height": h}),
    );
    let straightened = apply(&typed, straighten(identity, "bilinear", WINDOW));

    assert_eq!(straightened.dimensions(), cropped.dimensions());
    assert_eq!(placement(&straightened), placement(&cropped));
    assert!(
        same_rasters(&rasters(&straightened), &rasters(&cropped)),
        "an identity keeps every raster ALLOCATION, exactly as crop does"
    );
    assert!(
        same_rasters(&rasters(&straightened), &rasters(&typed)),
        "and they are the input's own buffers"
    );
    assert_eq!(mask_addresses(&straightened), mask_addresses(&cropped));
    assert_eq!(mask_addresses(&straightened), mask_addresses(&typed));
    assert_eq!(
        straightened.channels()[0].plane().samples(),
        cropped.channels()[0].plane().samples(),
        "the channel lands on exactly the crop's samples"
    );
    assert_eq!(pairs(straightened.guides()), pairs(cropped.guides()));
    assert_eq!(straightened.ruler_origin(), cropped.ruler_origin());
    assert_eq!(straightened.resolution(), typed.resolution());

    // The identity at the FULL window changes nothing at all.
    assert!(
        try_apply(&typed, straighten(identity, "bilinear", (0, 0, W, H)))
            .expect("an identity is not an error")
            .is_none(),
        "identity + full window is an exact no-op"
    );
}

// -------------------------------- (b) an integer translation shares planes --

#[test]
fn an_integer_translation_shares_allocations_and_matches_the_byte_twin() {
    let typed = typed_fixture();
    let byte = byte_fixture();
    let matrix = [1.0, 0.0, 0.0, 1.0, 4.0, -3.0];
    let window = (2, 1, 20, 15);

    let out = apply(&typed, straighten(matrix, "bilinear", window));
    assert_eq!(out.dimensions(), (window.2, window.3));
    assert!(
        same_rasters(&rasters(&out), &rasters(&typed)),
        "an integer translation re-points every raster buffer and never copies it"
    );
    assert_eq!(mask_addresses(&out), mask_addresses(&typed));
    // Every offset moved by the translation MINUS the window origin.
    let (dx, dy) = (4 - window.0 as i32, -3 - window.1 as i32);
    for (before, after) in placement(&typed).iter().zip(placement(&out)) {
        assert_eq!(before.0, after.0);
        assert_eq!(
            after.1,
            before.1.map(|(x, y, w, h)| (x + dx, y + dy, w, h)),
            "{}'s raster",
            before.0
        );
        assert_eq!(
            after.2,
            before.2.map(|(x, y, w, h)| (x + dx, y + dy, w, h)),
            "{}'s mask",
            before.0
        );
    }

    let twin = byte_chain(&byte, matrix, image::imageops::FilterType::Triangle, window);
    for (index, name) in ["Base", "Small"].iter().enumerate() {
        let layer = named(&out, name);
        assert_eq!(
            layer.offset(),
            twin.layers[index].offset,
            "{name}'s offset equals the byte chain's"
        );
        assert_eq!(
            layer.mask().unwrap().properties().offset,
            twin.layers[index].mask_properties.offset,
            "{name}'s mask offset equals the byte chain's"
        );
    }
}

// --------------------------------------------- (c) CORE-ONLY: exactly 90° --

#[test]
fn an_exact_quarter_turn_equals_rotate90_then_crop_and_keeps_its_own_chrome() {
    // CORE-ONLY: the Crop tool clamps its angle to -45…45 and the `crop` MCP
    // tool refuses `abs(angle) > 45`, so no host path can send this matrix.
    let typed = typed_fixture();
    let matrix = [0.0, 1.0, -1.0, 0.0, f64::from(H), 0.0];
    // Inside BOTH the 24x18 canvas and the 18x24 one `rotate90` produces.
    let window = (2, 3, 14, 12);

    let turned = apply(&typed, json!({"op": "rotate90"}));
    let reference = apply(
        &turned,
        json!({"op":"crop","x":window.0,"y":window.1,"width":window.2,"height":window.3}),
    );
    let out = apply(&typed, straighten(matrix, "bilinear", window));

    assert_eq!(out.dimensions(), (window.2, window.3));
    assert_eq!(out.dimensions(), reference.dimensions());
    assert_eq!(
        placement(&out),
        placement(&reference),
        "every raster and mask extent equals rotate90-then-crop's"
    );
    for name in ["Base", "Small", "Inner"] {
        assert_eq!(
            samples_of(&out, name),
            samples_of(&reference, name),
            "{name}'s pixels are the permutation's, bit for bit"
        );
    }
    for i in 0..out.layer_count() {
        let (got, want) = (
            out.layer(i).unwrap().mask().map(|m| m.plane().samples()),
            reference
                .layer(i)
                .unwrap()
                .mask()
                .map(|m| m.plane().samples()),
        );
        assert_eq!(got, want, "mask {i}'s samples");
    }
    assert_eq!(
        out.channels()[0].plane().samples(),
        reference.channels()[0].plane().samples(),
        "the channel's samples"
    );

    // The chrome and the PPI are the two things that DIFFER, and deliberately:
    // a straighten's chrome is `cropped_*` of the ORIGINAL guides, and the PPI
    // is never transposed.
    let byte = byte_fixture();
    let hand = byte
        .crop(window.0, window.1, window.2, window.3)
        .expect("the byte crop of the ORIGINAL canvas");
    assert_eq!(
        pairs(out.guides()),
        pairs(&hand.guides),
        "the guides are cropped_guides of the ORIGINAL chrome"
    );
    assert_eq!(out.ruler_origin(), hand.ruler_origin);
    assert_eq!(
        out.resolution(),
        typed.resolution(),
        "a straighten NEVER transposes the PPI"
    );
    assert_ne!(
        reference.resolution(),
        typed.resolution(),
        "where rotate90 does, which is what makes that assertion mean something"
    );
}

// --------------------------------- (d) bilinear and bicubic under G7 --

#[test]
fn a_seven_and_a_half_degree_straighten_matches_the_files_own_resampler() {
    let typed = typed_fixture();
    let matrix = rotation(ANGLE, WINDOW);
    let shifted = windowed(matrix, WINDOW.0, WINDOW.1);
    let inverse = invert(shifted);

    for filter in ["bilinear", "bicubic"] {
        let out = apply(&typed, straighten(matrix, filter, WINDOW));
        assert_eq!(
            out.dimensions(),
            (WINDOW.2, WINDOW.3),
            "{filter}: the canvas"
        );

        for name in ["Base", "Small", "Inner"] {
            let index = (0..typed.layer_count())
                .find(|&i| typed.layer(i).unwrap().name() == name)
                .unwrap();
            let source = typed.layer_source(index).unwrap();
            let dims = source.dimensions();
            let offset = typed.layer(index).unwrap().offset();
            let want_extent = bounds(offset, dims, shifted);

            let layer = named(&out, name);
            let got = (0..out.layer_count())
                .find(|&i| out.layer(i).unwrap().name() == name)
                .and_then(|i| out.layer_source(i))
                .unwrap();
            assert_eq!(
                (
                    layer.offset().0,
                    layer.offset().1,
                    got.dimensions().0,
                    got.dimensions().1
                ),
                want_extent,
                "{filter}: {name}'s destination extent is this file's own bounds"
            );
            let reference = oracle_rgba(
                &source.to_rgba16().ok().unwrap(),
                dims,
                offset,
                want_extent,
                inverse,
                filter,
            );
            compare_rgba(
                &got.to_rgba16().ok().unwrap(),
                &reference,
                &format!("{filter}: {name}'s pixels"),
            );
        }

        // The LOUD hidden RGB never bleeds: `Small`'s transparent border
        // stores (65535, 0, 65535) and every destination sample that rounds to
        // transparent stores four zeros (asserted inside `compare_rgba`), so
        // this records that the case is not vacuous.
        let small = (0..out.layer_count())
            .find(|&i| out.layer(i).unwrap().name() == "Small")
            .and_then(|i| out.layer_source(i))
            .unwrap();
        let transparent = small
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .filter(|p| p[3] == 0)
            .count();
        assert!(
            transparent > 0,
            "{filter}: the rotated Small layer must have fully transparent samples"
        );
        let translucent = small
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .filter(|p| p[3] > 0 && p[3] < 65535)
            .count();
        assert!(
            translucent > 0,
            "{filter}: and translucent ones, or the edge case is vacuous"
        );
    }
}

// ------------------------------------------------- (e) EVERY mask moves --

#[test]
fn every_mask_kind_rides_the_straighten_against_the_files_own_reference() {
    let typed = typed_fixture();
    let matrix = rotation(ANGLE, WINDOW);
    let shifted = windowed(matrix, WINDOW.0, WINDOW.1);
    let inverse = invert(shifted);

    let kinds: Vec<(String, bool, u8)> = (0..typed.layer_count())
        .filter_map(|i| {
            let layer = typed.layer(i).unwrap();
            layer.mask().map(|m| {
                (
                    layer.name().to_string(),
                    m.properties().linked,
                    m.properties().outside_coverage,
                )
            })
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("Base".into(), true, 0),
            ("Small".into(), false, 0),
            ("Group".into(), true, 0),
            ("Tone".into(), false, 255),
        ],
        "the four mask kinds the claim names, including one at outside 255"
    );

    for filter in ["bilinear", "bicubic"] {
        let out = apply(&typed, straighten(matrix, filter, WINDOW));
        for i in 0..typed.layer_count() {
            let Some(mask) = typed.layer(i).unwrap().mask() else {
                continue;
            };
            let name = typed.layer(i).unwrap().name().to_string();
            let dims = mask.plane().dimensions();
            let offset = mask.properties().offset;
            let outside = u16::from(mask.properties().outside_coverage) * 257;
            let want_extent = bounds(offset, dims, shifted);
            let got = out
                .layer(i)
                .unwrap()
                .mask()
                .unwrap_or_else(|| panic!("{name} lost its mask"));
            assert_eq!(
                (
                    got.properties().offset.0,
                    got.properties().offset.1,
                    got.plane().dimensions().0,
                    got.plane().dimensions().1
                ),
                want_extent,
                "{filter}: {name}'s mask extent"
            );
            let reference = oracle_plane(
                mask.plane().samples(),
                dims,
                offset,
                want_extent,
                inverse,
                filter,
                outside,
            );
            compare_plane(
                got.plane().samples(),
                &reference,
                &format!("{filter}: {name}'s mask"),
            );
            assert_eq!(
                got.properties().outside_coverage,
                mask.properties().outside_coverage,
                "{filter}: {name}'s outside coverage is carried"
            );
            assert_eq!(
                got.properties().linked,
                mask.properties().linked,
                "{filter}: {name}'s link flag is carried — EVERY mask moves, linked or not"
            );
        }
    }
}

// ------------------------------------ (f) channels ride into the window --

#[test]
fn a_saved_channel_rides_the_straighten_straight_into_the_window() {
    let typed = typed_fixture();
    let matrix = rotation(ANGLE, WINDOW);
    let shifted = windowed(matrix, WINDOW.0, WINDOW.1);
    let inverse = invert(shifted);

    for filter in ["bilinear", "bicubic"] {
        let out = apply(&typed, straighten(matrix, filter, WINDOW));
        assert_eq!(out.channels().len(), 1);
        assert_eq!(
            out.channels()[0].plane().dimensions(),
            (WINDOW.2, WINDOW.3),
            "{filter}: a channel is canvas space and stays canvas-sized"
        );
        assert_eq!(out.channels()[0].name(), typed.channels()[0].name());
        let reference = oracle_plane(
            typed.channels()[0].plane().samples(),
            (W, H),
            (0, 0),
            (0, 0, WINDOW.2, WINDOW.3),
            inverse,
            filter,
            0,
        );
        compare_plane(
            out.channels()[0].plane().samples(),
            &reference,
            &format!("{filter}: the saved channel"),
        );
    }
}

// --------------------------------- (g) chrome and the byte twin's extents --

#[test]
fn the_chrome_is_cropped_star_and_every_extent_equals_the_byte_chain() {
    let typed = typed_fixture();
    let byte = byte_fixture();
    let matrix = rotation(ANGLE, WINDOW);
    let out = apply(&typed, straighten(matrix, "bilinear", WINDOW));

    // HAND TABLE: `cropped_guides` shifts by (-x, -y) and DROPS what left the
    // window; `cropped_origin` shifts and CLAMPS. WINDOW is (3, 2, 18, 13).
    assert_eq!(
        pairs(out.guides()),
        vec![(HORIZONTAL, 6.25), (VERTICAL, 16.5)],
        "hand-written: 8.25 - 2 and 19.5 - 3 survive; (HORIZONTAL, 1.0) and \
         (VERTICAL, 2.0) shifted to -1 and left the window"
    );
    assert_eq!(
        out.ruler_origin(),
        (ORIGIN.0 - 3.0, ORIGIN.1 - 2.0),
        "hand-written: (4 - 3, 6 - 2), inside the 18 x 13 window"
    );

    let twin = byte_chain(&byte, matrix, image::imageops::FilterType::Triangle, WINDOW);
    assert_eq!(
        pairs(out.guides()),
        pairs(&twin.guides),
        "and the byte three-call chain says the same"
    );
    assert_eq!(out.ruler_origin(), twin.ruler_origin);
    assert_eq!((twin.width, twin.height), out.dimensions());

    for (index, name) in ["Base", "Small"].iter().enumerate() {
        let layer = named(&out, name);
        let source = (0..out.layer_count())
            .find(|&i| out.layer(i).unwrap().name() == *name)
            .and_then(|i| out.layer_source(i))
            .unwrap();
        assert_eq!(
            (layer.offset(), source.dimensions()),
            (
                twin.layers[index].offset,
                twin.layers[index].pixels.dimensions()
            ),
            "{name}'s destination extent equals the byte chain's exactly"
        );
        let mask = layer.mask().unwrap();
        assert_eq!(
            (mask.properties().offset, mask.plane().dimensions()),
            (
                twin.layers[index].mask_properties.offset,
                twin.layers[index].mask.as_ref().unwrap().dimensions()
            ),
            "{name}'s mask extent equals the byte chain's exactly"
        );
    }
    assert_eq!(
        out.channels()[0].plane().dimensions(),
        twin.channels[0].data.dimensions(),
        "and so does the channel's"
    );
}

// ----------------------- (h) style, descriptions, locks, linear light --

#[test]
fn the_style_factor_descriptions_locks_and_linear_light_follow_the_byte_rules() {
    let shadow = r##"{"effects":[{"type":"drop_shadow","use_global_light":false,
        "angle":90,"distance":5,"size":5,"spread":0,"opacity":0.75,"color":"#000000"}]}"##;
    let style = LayerStyle::from_json(shadow).expect("a style");

    let inner = Layer::raster("Inner", image(6, 5, 9000), (2, 2), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Group", vec![inner], 1.0, true)
        .unwrap()
        .with_layer_style(Some(&style), (W, H))
        .unwrap();
    let base = Layer::raster("Base", image(W, H, 100), (0, 0), 1.0, true)
        .unwrap()
        .with_layer_style(Some(&style), (W, H))
        .unwrap();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![base, group],
    )
    .expect("a styled document");

    // A ×2 matrix: `sqrt(|det|)` = 2 on the RASTER record, 1 on the group.
    let out = apply(
        &doc,
        straighten([2.0, 0.0, 0.0, 2.0, 0.0, 0.0], "bilinear", (0, 0, W, H)),
    );
    assert_eq!(
        effect_size(&out, "Base"),
        (10.0, 10.0),
        "Scale Effects at 2x"
    );
    assert_eq!(
        effect_size(&out, "Group"),
        (5.0, 5.0),
        "a GROUP has no pixels to resample, so its style is untouched"
    );

    // A DESCRIPTION is dropped by the core: the pixels it described moved.
    let described = {
        let description =
            CString::new(r#"{"type":"shape","version":3,"probe":"straighten"}"#).unwrap();
        let name = CString::new("Base").unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let source = doc.layer_source(0).unwrap();
        let ptr = unsafe {
            doctored_core::ffi_artwork::dr_precision_doc_artwork(
                &doc,
                0,
                true,
                source,
                0,
                0,
                name.as_ptr(),
                description.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!ptr.is_null(), "the artwork publication is admitted");
        *unsafe { Box::from_raw(ptr) }
    };
    assert!(described.artwork_description(0).is_some());
    let rotated = apply(
        &described,
        straighten(rotation(ANGLE, (0, 0, W, H)), "bilinear", (0, 0, W, H)),
    );
    assert!(
        rotated.artwork_description(0).is_none(),
        "a resampled raster drops its description"
    );

    // LOCK-EXEMPT (S4): a Lock All layer straightens, as the byte twin does.
    let typed = typed_fixture();
    let locked = edit(
        &typed,
        json!({"op": "layer_lock", "layer": 0, "locks": 0xF}),
    );
    assert_eq!(
        locked.layer(0).unwrap().locks(),
        doctored_core::doc_lock::LOCK_ALL,
        "reserved bits are masked off; Lock All is the three flags"
    );
    assert!(try_apply(
        &locked,
        straighten(rotation(ANGLE, WINDOW), "bilinear", WINDOW)
    )
    .expect("a locked layer does not refuse a whole-canvas op")
    .is_some());

    // 11E (S6): a `linear_light` document straightens and stays linear light.
    let linear = typed
        .with_evaluation(Evaluation::LinearLight)
        .expect("the switch is admitted")
        .expect("it is a change");
    let straightened = apply(
        &linear,
        straighten(rotation(ANGLE, WINDOW), "bilinear", WINDOW),
    );
    assert_eq!(straightened.evaluation(), Evaluation::LinearLight);
}

fn effect_size(doc: &PrecisionDocument, name: &str) -> (f32, f32) {
    match named(doc, name)
        .style()
        .unwrap_or_else(|| panic!("{name} has no style"))
        .effect(EffectKind::DropShadow)
    {
        Some(Effect::DropShadow(d)) => (d.size, d.distance),
        other => panic!("{name}: no drop shadow: {other:?}"),
    }
}

// ------------------------------------------------------- (i) refusals --

#[test]
fn every_refusal_states_its_own_sentence() {
    let typed = typed_fixture();
    let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

    // PAST THE PIXEL LIMIT (a window past the canvas is a crop past
    // the canvas, so only an over-limit window is refused for its size).
    let over = (0, 0, 20_000, 6_000);
    assert_eq!(
        refusal(&typed, straighten(identity, "bilinear", over)),
        "precision prototype dimensions exceed the supported pixel limit"
    );

    // ZERO SIZE.
    for window in [(2, 2, 0, 5), (2, 2, 5, 0)] {
        assert!(
            refusal(&typed, straighten(identity, "bilinear", window))
                .contains("dimensions exceed the supported pixel limit"),
            "a zero-size window: {window:?}"
        );
    }

    // NON-FINITE — built as a value rather than through JSON, because
    // `serde_json` writes an infinity as `null` and the request would be
    // refused by the PARSER instead of by the op.
    let nonfinite = GeometryEdit::Straighten {
        matrix: [1.0, 0.0, 0.0, f64::INFINITY, 0.0, 0.0],
        filter: doctored_core::precision_sampling::GeometryFilter::Bilinear,
        x: WINDOW.0 as i32,
        y: WINDOW.1 as i32,
        width: WINDOW.2,
        height: WINDOW.3,
        interactive: false,
        delete_cropped: false,
        background: None,
    };
    assert_eq!(
        typed.geometry(&nonfinite, LIMIT, 0, None).err(),
        Some("typed affine matrix must be finite".to_string())
    );
    // SINGULAR.
    assert_eq!(
        refusal(
            &typed,
            straighten([0.0, 0.0, 0.0, 0.0, 0.0, 0.0], "bilinear", WINDOW)
        ),
        "typed affine matrix is singular"
    );

    // A RETAINED SOURCE refuses, through `geometry_plan`'s existing arm.
    let master = PrecisionDocument::from_parts(
        6,
        4,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Original", image(6, 4, 500), (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let place = doctored_core::retained_source::SourceEdit::parse(
        &json!({"op":"place","source_id":"straighten-master","revision_id":"v1",
                "name":"Retained","x":2,"y":1})
        .to_string(),
    )
    .unwrap();
    let placed = typed
        .source_edit_input(
            Some(doctored_core::retained_source::SourceInput::Typed(&master)),
            &place,
            LIMIT,
            0,
            None,
        )
        .expect("the placement runs")
        .expect("it is a change");
    let straightened = apply(
        &placed,
        straighten(rotation(ANGLE, WINDOW), "bilinear", WINDOW),
    );
    assert_eq!(
        straightened.source_info(placed.layer_count() - 1).unwrap()["source_id"],
        "straighten-master"
    );

    // UNKNOWN and MISSING keys: `GeometryEdit` is a struct variant under
    // `deny_unknown_fields`, so all six keys are required and no seventh is.
    let mut request = straighten(identity, "bilinear", WINDOW);
    request["z"] = json!(1);
    assert!(GeometryEdit::parse(&request.to_string())
        .expect_err("an unknown key must refuse")
        .contains("invalid typed geometry request"));
    for key in ["matrix", "filter", "x", "y", "width", "height"] {
        let mut request = straighten(identity, "bilinear", WINDOW);
        request.as_object_mut().unwrap().remove(key);
        assert!(
            GeometryEdit::parse(&request.to_string())
                .expect_err("a missing key must refuse")
                .contains("invalid typed geometry request"),
            "the {key} key is REQUIRED"
        );
    }
}

// ---------------------------------------------------------- (j) float --

fn float_fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    Some(PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).expect("the fixture"))
}

/// The operator table a host reads admits `geometry_straighten`.
fn assert_float_straighten_admitted() {
    let table: Value =
        serde_json::from_str(&doctored_core::float_document::float_operators_json()).unwrap();
    assert!(
        table["admitted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "geometry_straighten"),
        "geometry_straighten is admitted at float: {table}"
    );
}

#[test]
fn a_float_document_admits_straighten() {
    let Some(base) = float_fixture() else {
        return;
    };
    assert_float_straighten_admitted();
    let (w, h) = base.dimensions();
    let window = (1, 1, w - 2, h - 2);
    let edit = parse(straighten(rotation(ANGLE, window), "bilinear", window));
    base.geometry_plan(&edit)
        .unwrap_or_else(|e| panic!("geometry_plan admits a float straighten: {e}"));
    let out = base
        .geometry(&edit, LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("geometry admits a float straighten: {e}"))
        .expect("a straighten changes the document");
    assert!(out.is_float());
    assert_eq!(
        out.dimensions(),
        (window.2, window.3),
        "the window is the canvas"
    );
}

// ------------------------------------------- (k) persistence and purity --

#[test]
fn native_save_reopen_and_resave_is_byte_identical_and_the_input_is_untouched() {
    let typed = typed_fixture();
    let before = placement(&typed);
    let before_rasters = rasters(&typed);
    let out = apply(
        &typed,
        straighten(rotation(ANGLE, WINDOW), "bicubic", WINDOW),
    );

    // PURITY: the input document is untouched, which is what makes the host's
    // undo stack one handle per step.
    assert_eq!(placement(&typed), before);
    assert!(same_rasters(&rasters(&typed), &before_rasters));
    assert_eq!(typed.dimensions(), (W, H));

    let bytes = out.encode_native(LIMIT, 0, None).expect("native bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(reopened.dimensions(), (WINDOW.2, WINDOW.3));
    assert_eq!(placement(&reopened), placement(&out), "extents survive");
    assert_eq!(
        reopened.channels()[0].plane().samples(),
        out.channels()[0].plane().samples(),
        "the resampled channel survives"
    );
    assert_eq!(pairs(reopened.guides()), pairs(out.guides()));
    assert_eq!(reopened.ruler_origin(), out.ruler_origin());
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("resave"),
        bytes,
        "reopen then resave is byte-identical"
    );
}

// ------------------------------------------ (l) the FFI and the tables --

#[test]
fn the_op_through_the_ffi_equals_the_rust_api_and_the_family_targets_the_document() {
    let typed = typed_fixture();
    let request = straighten(rotation(ANGLE, WINDOW), "bilinear", WINDOW);
    let through_ffi = edit(&typed, request.clone());
    let direct = apply(&typed, request);
    assert_eq!(through_ffi.dimensions(), direct.dimensions());
    assert_eq!(placement(&through_ffi), placement(&direct));
    for name in ["Base", "Small", "Inner"] {
        assert_eq!(samples_of(&through_ffi, name), samples_of(&direct, name));
    }
    assert_eq!(
        through_ffi.channels()[0].plane().samples(),
        direct.channels()[0].plane().samples()
    );
    assert_eq!(pairs(through_ffi.guides()), pairs(direct.guides()));

    let supported = capability("geometry_straighten", "document", 16);
    assert_eq!(
        supported["supported"], true,
        "geometry_straighten targets the document at 16 bits: {supported}"
    );
    let raster = capability("geometry_straighten", "raster", 16);
    assert_eq!(
        raster["supported"], false,
        "its only target is the document: {raster}"
    );
    let float = capability("geometry_straighten", "document", 32);
    assert_eq!(
        float["supported"], true,
        "the filtered straighten is admitted at 32-bit float: {float}"
    );
}
