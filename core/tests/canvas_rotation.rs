//! Phase 12F-1b3: DESTRUCTIVE whole-canvas rotation by any finite angle at
//! BYTE depth — `doc_rotate::{rotation_kind, rotated_canvas}` and
//! `dr_doc_geometry_edit`'s byte `rotate_canvas` request.
//!
//! Two independent yardsticks, never one answer compared with itself:
//!
//! * the FILTERED cases are measured against the shared f64 inverse-mapping
//!   reference (`common/inverse_mapping.rs`, at a maximum code of 255) and
//!   against hand-written destination extents — and the EXTENT TABLE itself
//!   is checked against this file's OWN restatement of R2's box, not against
//!   `bounds`, so the table and the implementation are derived twice;
//! * the EXACT cases are compared against the shipped ops they must equal
//!   (`rotate90` / `rotate180` / `rotate270`) BIT FOR BIT — every raster,
//!   every mask, every channel, every offset, the canvas, the guides, the
//!   ruler origin and the transposed PPI — on an even canvas AND on one whose
//!   `W + H` is odd, which is the case R2 alone would get wrong.
//!
//! TOLERANCES (fixed before measurement, never widened, all at 255):
//!
//! * exact cases: bit for bit;
//! * filtered vs the shared reference: alpha ≤ 1 code EVERYWHERE; straight
//!   RGB ≤ 1 code where the reference's alpha code ≥ 1; stored RGBA == 0
//!   where the result's own alpha rounds to 0 — which is what proves the LOUD
//!   hidden RGB at alpha 0 never bleeds;
//! * masks and channels: ≤ 1 code everywhere.

use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_comps::CompRequest;
use doctored_core::doc_guide::{Guide, GuideOrientation};
use doctored_core::doc_lock::LOCK_ALL;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::doc_rotate::{rotated_canvas, rotation_kind, RotationKind};
use doctored_core::ffi_geometry::{dr_doc_geometry_edit, dr_doc_geometry_edit_plan};
use doctored_core::ffi_rotate::dr_canvas_rotation_plan;
use doctored_core::metadata::Resolution;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::SourceEdit;
use doctored_core::style::LayerStyle;
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::json;

use crate::inverse_mapping;

use inverse_mapping::{
    assert_plane_within_one, assert_rgba_within_one, bounds, invert, reference_plane,
    reference_rgba,
};

/// The maximum code of byte storage — every comparison in this file.
const MAX_CODE: u32 = 255;

const W: u32 = 64;
const H: u32 = 48;
/// The FILTERED angle every pixel comparison uses.
const ANGLE: f64 = 30.0;

const HORIZONTAL: GuideOrientation = GuideOrientation::Horizontal;
const VERTICAL: GuideOrientation = GuideOrientation::Vertical;

const GUIDES: [(GuideOrientation, f64); 4] = [
    (VERTICAL, 2.0),
    (VERTICAL, 50.5),
    (HORIZONTAL, 1.0),
    (HORIZONTAL, 8.25),
];
const RULER_ORIGIN: (f64, f64) = (7.0, 5.0);
const PPI: Resolution = Resolution { x: 300.0, y: 150.0 };

const SMALL_AT: (i32, i32) = (20, 14);
const SMALL_DIMS: (u32, u32) = (9, 7);

const NEAREST: &str = "nearest";
const BILINEAR: &str = "bilinear";
const BICUBIC: &str = "bicubic";

// ------------------------------------- this file's OWN restatement of R2 --

/// R2's box, written out here from the definition rather than read from the
/// core: rotate the four canvas corners about `(W/2, H/2)`, snap each onto an
/// integer it sits within 1e-9 of, take the extent (snapped once more) and
/// round it UP — `ceil(bw) × ceil(bh)` — and place the old
/// canvas's top-left at `(⌊(W′−W)/2⌋, ⌊(H′−H)/2⌋)`, so `left`/`top` are minus
/// that. The EXTENT TABLE is asserted against THIS, so the table and
/// `rotated_canvas` are two derivations of one rule.
fn own_box(width: u32, height: u32, degrees: f64) -> (i32, i32, u32, u32) {
    let mut value = degrees % 360.0;
    if value <= -180.0 {
        value += 360.0;
    }
    if value > 180.0 {
        value -= 360.0;
    }
    let radians = value.to_radians();
    let (sin, cos) = radians.sin_cos();
    let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
    let tx = cx - cos * cx + sin * cy;
    let ty = cy - sin * cx - cos * cy;
    let at = |x: f64, y: f64| (cos * x - sin * y + tx, sin * x + cos * y + ty);
    let snap = |v: f64| {
        let r = v.round();
        if (v - r).abs() <= 1e-9 {
            r
        } else {
            v
        }
    };
    let (w, h) = (f64::from(width), f64::from(height));
    let corners = [at(0.0, 0.0), at(w, 0.0), at(0.0, h), at(w, h)];
    let xs: Vec<f64> = corners.iter().map(|c| snap(c.0)).collect();
    let ys: Vec<f64> = corners.iter().map(|c| snap(c.1)).collect();
    let extent = |vs: &[f64]| {
        let min = vs.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = vs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        snap(max - min).ceil() as i64
    };
    let (new_w, new_h) = (extent(&xs), extent(&ys));
    let floor_half = |grown: i64, old: u32| (grown - i64::from(old)).div_euclid(2);
    (
        -floor_half(new_w, width) as i32,
        -floor_half(new_h, height) as i32,
        new_w as u32,
        new_h as u32,
    )
}

// ------------------------------------------------------------- fixtures --

fn image(w: u32, h: u32, seed: u8) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        let v = seed.wrapping_add((i * 37) as u8);
        Rgba([v, v.wrapping_mul(3), 255 - v, 255])
    })
}

/// The layer the pixel comparison is really about: a translucent one-pixel
/// ring inside a fully TRANSPARENT border whose stored RGB is LOUD magenta.
/// Premultiplied accumulation must weight that colour by zero, so it can
/// never bleed into a neighbouring sample.
fn edge_image(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        let v = (37u8).wrapping_add((i * 91) as u8);
        if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
            Rgba([255, 0, 255, 0])
        } else if x == 1 || y == 1 || x == w - 2 || y == h - 2 {
            Rgba([v, v.wrapping_mul(5), 255 - v, 128])
        } else {
            Rgba([v, v.wrapping_mul(3), 255 - v, 255])
        }
    })
}

fn plane_samples(w: u32, h: u32, seed: u32) -> Vec<u8> {
    (0..w * h).map(|i| ((i * 7 + seed) % 251) as u8).collect()
}

fn mask_at(
    w: u32,
    h: u32,
    offset: (i32, i32),
    linked: bool,
    seed: u32,
    outside: u8,
) -> MaskProperties {
    let mut properties = MaskProperties::at(offset);
    properties.linked = linked;
    properties.outside_coverage = outside;
    let _ = (w, h, seed);
    properties
}

/// The BYTE fixture, on a `width` x `height` canvas:
///
/// * `Base`, a canvas-sized raster with a canvas-sized LINKED mask at (2, −1);
/// * `Small`, the translucent-edged raster at a non-zero offset with an
///   UNLINKED mask of its own;
/// * `Group`, an enclosing group carrying a canvas-sized linked mask;
/// * `Tone`, an adjustment layer (a raster whose `meta` is an op) with an
///   UNLINKED mask whose OUTSIDE coverage is 255 — the one mask whose
///   untouched region is opaque;
/// * `Locked`, a Lock-All raster, because the op is lock-EXEMPT;
/// * `Styled`, a raster carrying a KNOCKOUT style, whose `Arc` must survive;
///
/// plus one saved channel, four guides, a moved ruler origin, 300 x 150 ppi
/// and one layer comp.
fn fixture(width: u32, height: u32) -> DrDocument {
    let mut doc = DrDocument::from_pixels(image(width, height, 11));
    doc.layers[0].name = "Base".into();
    doc.layers[0].mask = Some(Arc::new(
        GrayImage::from_raw(width, height, plane_samples(width, height, 1)).unwrap(),
    ));
    doc.layers[0].mask_properties = mask_at(width, height, (2, -1), true, 1, 0);

    doc = doc
        .adding_image_layer(0, edge_image(SMALL_DIMS.0, SMALL_DIMS.1), "Small")
        .expect("the Small layer");
    doc.layers[1].offset = SMALL_AT;
    doc.layers[1].mask = Some(Arc::new(
        GrayImage::from_raw(
            SMALL_DIMS.0,
            SMALL_DIMS.1,
            plane_samples(SMALL_DIMS.0, SMALL_DIMS.1, 2),
        )
        .unwrap(),
    ));
    doc.layers[1].mask_properties = mask_at(SMALL_DIMS.0, SMALL_DIMS.1, SMALL_AT, false, 2, 0);

    // The adjustment layer: a raster whose meta parses as an op. The core
    // keeps it, because that meta describes an OPERATION and not pixels.
    doc = doc
        .adding_image_layer(1, RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0])), "Tone")
        .expect("the Tone layer");
    doc.layers[2].meta = Some(json!({"type":"adjust","op":"invert","params":{}}).to_string());
    doc.layers[2].mask = Some(Arc::new(
        GrayImage::from_raw(width, height, plane_samples(width, height, 4)).unwrap(),
    ));
    doc.layers[2].mask_properties = mask_at(width, height, (-2, 4), false, 4, 255);

    doc = doc
        .adding_image_layer(2, image(11, 9, 200), "Locked")
        .expect("the Locked layer");
    doc.layers[3].offset = (5, 30);
    doc = doc
        .with_layer_locks(3, LOCK_ALL)
        .expect("the Lock All flags");

    doc = doc
        .adding_image_layer(3, image(13, 11, 90), "Styled")
        .expect("the Styled layer");
    doc.layers[4].offset = (40, 4);
    let knockout = r##"{"fill_opacity":0.5,"knockout":"shallow","effects":[{"type":"drop_shadow",
        "use_global_light":false,"angle":90,"distance":5,"size":5,"spread":0,"opacity":0.75,
        "color":"#000000"}]}"##;
    let style = LayerStyle::from_json(knockout).expect("a knockout style");
    doc = doc
        .set_layer_style(4, Some(Arc::new(style)))
        .expect("the style attaches");

    // The GROUP, around `Small` alone, carrying a canvas-sized linked mask.
    let (mut grouped, group, _, _) = doc.group_layers(&[1], "Group").expect("the group");
    grouped.layers[group].mask = Some(Arc::new(
        GrayImage::from_raw(width, height, plane_samples(width, height, 3)).unwrap(),
    ));
    grouped.layers[group].mask_properties = mask_at(width, height, (0, 0), true, 3, 0);
    grouped.layers[group].offset = (0, 0);
    let mut doc = grouped;

    doc = doc
        .add_channel(
            "Coverage",
            &plane_samples(width, height, 11),
            width,
            height,
            [255, 0, 0],
            0.5,
        )
        .expect("a channel");
    for (orientation, position) in GUIDES {
        doc = doc.add_guide(orientation, position).expect("a guide");
    }
    doc = doc
        .set_ruler_origin(RULER_ORIGIN.0, RULER_ORIGIN.1)
        .expect("the origin moves");
    doc.resolution = PPI;
    let request = CompRequest::parse(
        &json!({"op":"layer_comp","action":"save","name":"Shot","visibility":true}).to_string(),
    )
    .expect("a comp request");
    doc.edit_layer_comps(&request)
        .expect("the comp saves")
        .expect("saving a comp is a change")
}

// ----------------------------------------------------------- comparisons --

fn rgba_samples(image: &RgbaImage) -> Vec<[u32; 4]> {
    image
        .as_raw()
        .chunks_exact(4)
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

fn plane_values(plane: &GrayImage) -> Vec<u32> {
    plane.as_raw().iter().map(|&v| u32::from(v)).collect()
}

fn guide_pairs(guides: &[Guide]) -> Vec<(GuideOrientation, f64)> {
    guides.iter().map(|g| (g.orientation, g.position)).collect()
}

fn named(doc: &DrDocument, name: &str) -> usize {
    (0..doc.layers.len())
        .find(|&i| doc.layers[i].name == name)
        .unwrap_or_else(|| panic!("no layer named {name}"))
}

/// Every layer's `(name, offset, pixel dims, mask offset, mask dims)` — the
/// whole placement of the document in one comparable value.
type Placement = (
    String,
    (i32, i32),
    (u32, u32),
    Option<((i32, i32), (u32, u32))>,
);

fn placement(doc: &DrDocument) -> Vec<Placement> {
    doc.layers
        .iter()
        .map(|l| {
            (
                l.name.clone(),
                l.offset,
                l.pixels.dimensions(),
                l.mask
                    .as_ref()
                    .map(|m| (l.mask_properties.offset, m.dimensions())),
            )
        })
        .collect()
}

/// The bit-for-bit comparison the EXACT cases use: nothing is sampled by
/// either side, so every buffer must match byte for byte.
fn assert_identical(got: &DrDocument, want: &DrDocument, what: &str) {
    assert_eq!(
        (got.width, got.height),
        (want.width, want.height),
        "{what}: canvas"
    );
    assert_eq!(placement(got), placement(want), "{what}: placement");
    assert_eq!(got.layers.len(), want.layers.len(), "{what}: layer count");
    for (index, (g, w)) in got.layers.iter().zip(&want.layers).enumerate() {
        assert_eq!(
            g.pixels.contiguous_image().as_raw(),
            w.pixels.contiguous_image().as_raw(),
            "{what}: layer {index}'s pixels"
        );
        assert_eq!(
            g.mask.as_ref().map(|m| m.as_raw().clone()),
            w.mask.as_ref().map(|m| m.as_raw().clone()),
            "{what}: layer {index}'s mask"
        );
        assert!(
            g.mask_properties == w.mask_properties,
            "{what}: layer {index}'s mask properties"
        );
        assert_eq!(g.id, w.id, "{what}: layer {index}'s id");
        assert_eq!(g.locks, w.locks, "{what}: layer {index}'s locks");
        assert_eq!(g.meta, w.meta, "{what}: layer {index}'s meta");
    }
    assert_eq!(
        got.channels.len(),
        want.channels.len(),
        "{what}: channel count"
    );
    for (index, (g, w)) in got.channels.iter().zip(&want.channels).enumerate() {
        assert_eq!(g.name, w.name, "{what}: channel {index}'s name");
        assert_eq!(
            g.data.as_raw(),
            w.data.as_raw(),
            "{what}: channel {index}'s samples"
        );
    }
    assert_eq!(
        guide_pairs(&got.guides),
        guide_pairs(&want.guides),
        "{what}: guides"
    );
    assert_eq!(got.ruler_origin, want.ruler_origin, "{what}: ruler origin");
    assert_eq!(got.resolution, want.resolution, "{what}: ppi");
}

/// A filtered or quarter rotation through the host's route
/// (`ffi_rotate_canvas`), which must answer a document.
fn rotated(doc: &DrDocument, degrees: f64, filter: &str) -> DrDocument {
    let (handle, err) = ffi_rotate_canvas(doc, degrees, filter);
    assert!(
        !handle.is_null(),
        "rotate_canvas({degrees}) refused: {}",
        take(err)
    );
    *unsafe { Box::from_raw(handle) }
}

/// The owned text of an `err_out` string, freed; empty for NULL.
fn take(err: *mut std::ffi::c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let text = unsafe { std::ffi::CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { doctored_core::ffi::dr_string_free(err) };
    text
}

/// The host's route: `dr_doc_geometry_edit`'s byte `rotate_canvas` request
/// (`filter` is its JSON name), NULL job control. Returns the handle and the
/// raw `err_out`.
fn ffi_rotate_canvas(
    doc: *const DrDocument,
    degrees: f64,
    filter: &str,
) -> (*mut DrDocument, *mut std::ffi::c_char) {
    let request = std::ffi::CString::new(
        json!({"op": "rotate_canvas", "degrees": degrees, "filter": filter}).to_string(),
    )
    .unwrap();
    let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
    let handle = unsafe {
        dr_doc_geometry_edit(
            doc,
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    (handle, err)
}

/// A rotation the FFI must refuse: NULL WITH a sentence, which is returned.
fn refusal(doc: &DrDocument, degrees: f64, filter: &str) -> String {
    let (handle, err) = ffi_rotate_canvas(doc, degrees, filter);
    assert!(handle.is_null(), "rotate_canvas({degrees}) must be refused");
    assert!(
        !err.is_null(),
        "rotate_canvas({degrees}) must name its refusal"
    );
    take(err)
}

// ------------------------------------------ (a) what an angle CLASSIFIES --

#[test]
fn every_angle_classifies_and_normalizes_by_the_one_rule() {
    use RotationKind::*;
    let table: [(f64, RotationKind, f64); 14] = [
        (0.0, Identity, 0.0),
        (-0.0, Identity, 0.0),
        (360.0, Identity, 0.0),
        (-720.0, Identity, 0.0),
        (90.0, Rotate90, 90.0),
        (450.0, Rotate90, 90.0),
        (-270.0, Rotate90, 90.0),
        (90.0 + 1e-12, Rotate90, 90.0 + 1e-12),
        (180.0, Rotate180, 180.0),
        (-180.0, Rotate180, 180.0),
        (540.0, Rotate180, 180.0),
        (270.0, Rotate270, -90.0),
        (-90.0, Rotate270, -90.0),
        (187.5, Filtered, -172.5),
    ];
    for (degrees, kind, normalized) in table {
        let (got_kind, got_degrees) =
            rotation_kind(degrees).unwrap_or_else(|| panic!("{degrees} must classify"));
        assert_eq!(got_kind, kind, "{degrees} classifies");
        assert!(
            (got_degrees - normalized).abs() <= 1e-9,
            "{degrees} normalizes to {got_degrees}, expected {normalized}"
        );
        assert!(
            got_degrees > -180.0 && got_degrees <= 180.0,
            "{degrees} normalized outside (-180, 180]: {got_degrees}"
        );
        // A -0 would be the one number a caller could not compare with 0.
        assert!(
            got_degrees != 0.0 || got_degrees.is_sign_positive(),
            "{degrees} normalized to a negative zero"
        );
    }
    for degrees in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            rotation_kind(degrees).is_none(),
            "{degrees} must not classify"
        );
        assert!(rotated_canvas(W, H, degrees).is_none());
    }
}

// ------------------------------------------------- (b) the EXTENT TABLE --

#[test]
fn the_extent_table_holds_against_this_files_own_box() {
    #[allow(clippy::type_complexity)]
    let table: [(u32, u32, f64, (i32, i32, u32, u32)); 31] = [
        (64, 48, 7.5, (-3, -4, 70, 56)),
        (64, 48, -7.5, (-3, -4, 70, 56)),
        (64, 48, 187.5, (-3, -4, 70, 56)),
        (64, 48, 30.0, (-8, -13, 80, 74)),
        (63, 47, 30.0, (-8, -13, 79, 73)),
        (64, 48, 45.0, (-8, -16, 80, 80)),
        (63, 48, 45.0, (-8, -15, 79, 79)),
        (1, 1, 45.0, (0, 0, 2, 2)),
        (100, 10, 80.0, (36, -45, 28, 101)),
        (100, 10, -80.0, (36, -45, 28, 101)),
        (10, 100, 80.0, (-45, 36, 101, 28)),
        (64, 48, 89.999, (8, -8, 49, 65)),
        (64, 48, 90.001, (8, -8, 49, 65)),
        (64, 48, -90.001, (8, -8, 49, 65)),
        (63, 48, 90.001, (7, -8, 49, 64)),
        (64, 48, 0.001, (0, 0, 65, 49)),
        (64, 48, 1e-6, (0, 0, 65, 49)),
        (64, 48, 180.001, (0, 0, 65, 49)),
        (48, 36, 7.5, (-2, -3, 53, 42)),
        (48, 36, 30.0, (-6, -10, 60, 56)),
        (48, 36, 89.999, (6, -6, 37, 49)),
        (40, 28, 12.5, (-3, -4, 46, 36)),
        // Photoshop take1's rows (`TAKE_ROWS` below holds Photoshop's sizes).
        (120, 80, 10.0, (-6, -10, 133, 100)),
        (120, 80, 45.0, (-11, -31, 142, 142)),
        (120, 80, 120.0, (-5, -32, 130, 144)),
        (120, 80, -20.0, (-10, -18, 141, 117)),
        (121, 81, 10.0, (-6, -10, 134, 101)),
        (121, 81, 45.0, (-11, -31, 143, 143)),
        (121, 81, 120.0, (-5, -32, 131, 146)),
        (121, 81, -20.0, (-10, -18, 142, 118)),
        (120, 80, 30.0, (-12, -25, 144, 130)),
    ];
    for (width, height, degrees, want) in table {
        // Every row is FILTERED: that is what makes the row about R2's box
        // rather than about a delegated quarter turn.
        assert_eq!(
            rotation_kind(degrees).map(|(kind, _)| kind),
            Some(RotationKind::Filtered),
            "{width}x{height} @ {degrees} must be a filtered angle"
        );
        assert_eq!(
            own_box(width, height, degrees),
            want,
            "{width}x{height} @ {degrees}: this file's own box"
        );
        let plan = rotated_canvas(width, height, degrees)
            .unwrap_or_else(|| panic!("{width}x{height} @ {degrees} must plan"));
        assert_eq!(
            (plan.left, plan.top, plan.width, plan.height),
            want,
            "{width}x{height} @ {degrees}: rotated_canvas"
        );
        // R2's origin: the centred placement, floored.
        assert_eq!(
            plan.origin(),
            (
                (plan.width as i64 - width as i64).div_euclid(2) as i32,
                (plan.height as i64 - height as i64).div_euclid(2) as i32
            ),
            "{width}x{height} @ {degrees}: the origin is (floor((W'-W)/2), floor((H'-H)/2))"
        );
    }

    // Photoshop's canvas on each of take1's rows: `rotate.json`'s
    // `size.<row>.photoshop` (TAKE2) and `photoshop.json`'s R30.
    let take2 = reference_fixture("rotate.json");
    let take1 = reference_fixture("photoshop.json");
    for (row, width, height, degrees) in TAKE_ROWS {
        let case = if row == "R30" {
            &take1["cases"]["R30"]
        } else {
            assert_eq!(
                take2["cases"][row]["angle"].as_f64(),
                Some(degrees),
                "{row}"
            );
            &take2["cases"][row]
        };
        assert_eq!(case["canvas_before"], json!([width, height]), "{row}");
        let photoshop = if row == "R30" {
            &case["canvas_after"]
        } else {
            &take2["size"][row]["photoshop"]
        };
        let photoshop = (
            photoshop[0].as_u64().expect("a width") as u32,
            photoshop[1].as_u64().expect("a height") as u32,
        );
        let plan = rotated_canvas(width, height, degrees).expect("a take row plans");
        assert_eq!(
            (plan.width, plan.height),
            photoshop,
            "{row}: {width}x{height} @ {degrees} is Photoshop's canvas"
        );
    }

    // Past the 100 MP pixel budget: the box is refused.
    assert!(rotated_canvas(30000, 3000, 45.0).is_none());
    // A zero side has no box at all.
    assert!(rotated_canvas(0, 10, 30.0).is_none());
    assert!(rotated_canvas(10, 0, 30.0).is_none());

    // R3's hazard, measured: R2 applied to an EXACT quarter turn of a
    // `W + H`-odd canvas gives rotate90's size but a HALF-PIXEL placement —
    // `[0, 1, −1, 0, 47.5, −0.5]` where rotate90 is `[0, 1, −1, 0, 48, 0]` —
    // which is why delegation is mandatory rather than an optimisation.
    let wrong = rotated_canvas(63, 48, 90.0).expect("the box still computes");
    assert_eq!((wrong.width, wrong.height), (48, 63));
    let want = [0.0, 1.0, -1.0, 0.0, 47.5, -0.5];
    assert!(
        wrong
            .matrix
            .iter()
            .zip(want)
            .all(|(got, want)| (got - want).abs() <= 1e-9),
        "R2's matrix on a W+H-odd quarter turn: {:?}",
        wrong.matrix
    );
    let rotate90 = [0.0, 1.0, -1.0, 0.0, 48.0, 0.0];
    assert!(
        (wrong.matrix[4] - rotate90[4]).abs() > 0.25
            && (wrong.matrix[5] - rotate90[5]).abs() > 0.25,
        "R2's box is NOT rotate90's placement on a W+H-odd canvas"
    );
}

/// The take1 rows: the name, the canvas and the angle (`rotate.json`'s
/// TAKE2 rows, plus `photoshop.json`'s R30).
const TAKE_ROWS: [(&str, u32, u32, f64); 9] = [
    ("R10", 120, 80, 10.0),
    ("R45", 120, 80, 45.0),
    ("R120", 120, 80, 120.0),
    ("RN20", 120, 80, -20.0),
    ("O10", 121, 81, 10.0),
    ("O45", 121, 81, 45.0),
    ("O120", 121, 81, 120.0),
    ("ON20", 121, 81, -20.0),
    ("R30", 120, 80, 30.0),
];

/// One of the reference-point fixtures.
fn reference_fixture(name: &str) -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/reference_point")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
        .expect("the fixture parses")
}

// --------------------------- (c) an exact quarter turn IS the shipped op --

#[test]
fn every_exact_quarter_turn_equals_the_shipped_op_bit_for_bit() {
    for (width, height) in [(W, H), (63, 48)] {
        let doc = fixture(width, height);
        let odd = (width + height) % 2 == 1;
        let cases: [(f64, DrDocument); 6] = [
            (90.0, doc.rotate90().unwrap()),
            (450.0, doc.rotate90().unwrap()),
            (180.0, doc.rotate180().unwrap()),
            (-180.0, doc.rotate180().unwrap()),
            (270.0, doc.rotate270().unwrap()),
            (-90.0, doc.rotate270().unwrap()),
        ];
        for (degrees, want) in cases {
            // The filter is deliberately Nearest here and Lanczos below: an
            // exact route never reads it.
            let got = rotated(&doc, degrees, NEAREST);
            assert_identical(&got, &want, &format!("{width}x{height} @ {degrees}"));
            let lanczos = rotated(&doc, degrees, "lanczos3");
            assert_identical(
                &lanczos,
                &want,
                &format!("{width}x{height} @ {degrees}, lanczos"),
            );
        }
        // The PPI is TRANSPOSED by a quarter turn and only by one.
        let quarter = rotated(&doc, 90.0, NEAREST);
        assert_eq!(
            quarter.resolution,
            Resolution { x: PPI.y, y: PPI.x },
            "a quarter turn transposes the ppi"
        );
        assert_eq!(
            rotated(&doc, 180.0, NEAREST).resolution,
            PPI,
            "a half turn does not"
        );
        if odd {
            // The whole point of R3 on this canvas.
            assert_eq!(
                (quarter.width, quarter.height),
                (height, width),
                "a W+H-odd canvas turns to exactly H x W"
            );
        }
    }
}

// ------------------------- (d) the filtered rasters against the reference --

#[test]
fn a_thirty_degree_rotation_matches_the_shared_reference_for_every_raster() {
    let doc = fixture(W, H);
    let plan = rotated_canvas(W, H, ANGLE).expect("the plan");
    let inverse = invert(plan.matrix);

    for kind in ["bilinear", "bicubic"] {
        let out = rotated(&doc, ANGLE, kind);
        assert_eq!(
            (out.width, out.height),
            (plan.width, plan.height),
            "{kind}: the canvas"
        );

        for name in ["Base", "Small", "Locked", "Styled"] {
            let index = named(&doc, name);
            let source = &doc.layers[index];
            let want_extent = bounds(source.offset, source.pixels.dimensions(), plan.matrix);
            let got_index = named(&out, name);
            let got = &out.layers[got_index];
            assert_eq!(
                (
                    got.offset.0,
                    got.offset.1,
                    got.pixels.dimensions().0,
                    got.pixels.dimensions().1
                ),
                want_extent,
                "{kind}: {name}'s destination extent"
            );
            let reference = reference_rgba(
                &rgba_samples(&source.pixels.contiguous_image()),
                source.pixels.dimensions(),
                source.offset,
                want_extent,
                inverse,
                kind,
                MAX_CODE,
            );
            assert_rgba_within_one(
                &rgba_samples(&got.pixels.contiguous_image()),
                &reference,
                &format!("{kind}: {name}'s pixels"),
            );
        }

        // The LOUD hidden RGB never bleeds: `Small`'s transparent border
        // stores (255, 0, 255) and every destination sample that rounds to
        // transparent stores four zeros (asserted inside the comparison), so
        // this records that the case is not vacuous.
        let small = &out.layers[named(&out, "Small")].pixels;
        let transparent = small
            .contiguous_image()
            .as_raw()
            .chunks_exact(4)
            .filter(|p| p[3] == 0)
            .count();
        let translucent = small
            .contiguous_image()
            .as_raw()
            .chunks_exact(4)
            .filter(|p| p[3] > 0 && p[3] < 255)
            .count();
        assert!(
            transparent > 0,
            "{kind}: the rotated Small layer must have fully transparent samples"
        );
        assert!(
            translucent > 0,
            "{kind}: and translucent ones, or the edge case is vacuous"
        );
        for pixel in small.contiguous_image().as_raw().chunks_exact(4) {
            if pixel[3] == 0 {
                assert_eq!(
                    pixel,
                    [0, 0, 0, 0],
                    "{kind}: a transparent sample kept a stored colour"
                );
            }
        }
    }
}

// ----------------------------------------------- (e) EVERY mask rides it --

#[test]
fn every_mask_kind_rides_the_rotation_against_the_shared_reference() {
    let doc = fixture(W, H);
    let plan = rotated_canvas(W, H, ANGLE).expect("the plan");
    let inverse = invert(plan.matrix);

    let kinds: Vec<(String, bool, u8)> = doc
        .layers
        .iter()
        .filter(|l| l.mask.is_some())
        .map(|l| {
            (
                l.name.clone(),
                l.mask_properties.linked,
                l.mask_properties.outside_coverage,
            )
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("Base".to_string(), true, 0),
            ("Small".to_string(), false, 0),
            ("Group".to_string(), true, 0),
            ("Tone".to_string(), false, 255),
        ],
        "the four mask kinds the claim names, including one at outside 255"
    );

    for kind in ["bilinear", "bicubic"] {
        let out = rotated(&doc, ANGLE, kind);
        for source in doc.layers.iter() {
            let Some(mask) = &source.mask else { continue };
            let name = &source.name;
            let offset = source.mask_properties.offset;
            let outside = u32::from(source.mask_properties.outside_coverage);
            let want_extent = bounds(offset, mask.dimensions(), plan.matrix);
            let got = &out.layers[named(&out, name)];
            let got_mask = got
                .mask
                .as_ref()
                .unwrap_or_else(|| panic!("{name} lost its mask"));
            assert_eq!(
                (
                    got.mask_properties.offset.0,
                    got.mask_properties.offset.1,
                    got_mask.dimensions().0,
                    got_mask.dimensions().1
                ),
                want_extent,
                "{kind}: {name}'s mask extent"
            );
            let reference = reference_plane(
                &plane_values(mask),
                mask.dimensions(),
                offset,
                want_extent,
                inverse,
                kind,
                outside,
                MAX_CODE,
            );
            assert_plane_within_one(
                &plane_values(got_mask),
                &reference,
                &format!("{kind}: {name}'s mask"),
            );
            assert_eq!(
                got.mask_properties.outside_coverage, source.mask_properties.outside_coverage,
                "{kind}: {name}'s outside coverage is carried"
            );
            assert_eq!(
                got.mask_properties.linked, source.mask_properties.linked,
                "{kind}: {name}'s link flag is carried — EVERY mask moves, linked or not"
            );
        }
    }
}

// ------------------------------- (f) channels ride straight into the canvas --

#[test]
fn a_saved_channel_rides_straight_into_the_new_canvas_even_when_it_shrinks() {
    for (width, height, degrees) in [(W, H, ANGLE), (100, 10, 80.0)] {
        let doc = fixture(width, height);
        let plan = rotated_canvas(width, height, degrees).expect("the plan");
        let inverse = invert(plan.matrix);
        for kind in ["bilinear", "bicubic"] {
            let out = rotated(&doc, degrees, kind);
            assert_eq!(out.channels.len(), 1);
            assert_eq!(
                out.channels[0].data.dimensions(),
                (plan.width, plan.height),
                "{kind}: a channel is canvas space and stays canvas-sized"
            );
            assert_eq!(out.channels[0].name, doc.channels[0].name);
            assert_eq!(
                out.channels[0].id, doc.channels[0].id,
                "{kind}: the channel keeps its identity"
            );
            let reference = reference_plane(
                &plane_values(&doc.channels[0].data),
                (width, height),
                (0, 0),
                (0, 0, plan.width, plan.height),
                inverse,
                kind,
                0,
                MAX_CODE,
            );
            assert_plane_within_one(
                &plane_values(&out.channels[0].data),
                &reference,
                &format!("{width}x{height} @ {degrees} {kind}: the saved channel"),
            );
        }
        if width == 100 {
            // R9's point: this axis SHRANK (100 -> 28) and no coverage that
            // lands inside the new canvas was lost — the comparison above is
            // against a straight walk into (0, 0, W', H'), not against a
            // transform-then-crop.
            assert!(plan.width < width, "the 100x10 case must shrink on x");
            assert!(plan.left > 0, "and its left must be positive");
        }
    }
}

// ------------------------- (g) the chrome, by hand, and the untransposed PPI --

#[test]
fn the_chrome_follows_padded_star_and_the_ppi_is_never_transposed() {
    // The SHRINKING case, because that is the only one that can DROP a guide.
    // 100 x 10 at 80 degrees is 28 x 101 with the old top-left at (-36, 45).
    let doc = fixture(100, 10);
    let plan = rotated_canvas(100, 10, 80.0).expect("the plan");
    assert_eq!(plan.origin(), (-36, 45));
    let out = rotated(&doc, 80.0, BILINEAR);
    assert_eq!((out.width, out.height), (28, 101));
    // HAND TABLE. The four guides are (V 2.0), (V 50.5), (H 1.0), (H 8.25).
    // Verticals shift by -36: 2 -> -34 (DROPPED, outside [0, 28]) and
    // 50.5 -> 14.5 (kept). Horizontals shift by +45: 1 -> 46 and
    // 8.25 -> 53.25, both inside [0, 101].
    assert_eq!(
        guide_pairs(&out.guides),
        vec![(HORIZONTAL, 46.0), (HORIZONTAL, 53.25), (VERTICAL, 14.5)],
        "hand-written: one vertical guide left the shrunken canvas"
    );
    // The ruler origin shifts and CLAMPS: (7 - 36, 5 + 45) = (-29, 50),
    // clamped into 28 x 101 on x.
    assert_eq!(out.ruler_origin, (0.0, 50.0));

    // The PPI: only an EXACT quarter turn transposes it. 89.999 degrees is
    // not one, and the discontinuity at 90 is disclosed rather than smoothed.
    let square = fixture(W, H);
    assert_eq!(
        rotated(&square, 89.999, BILINEAR).resolution,
        PPI,
        "89.999 degrees leaves the ppi where it was"
    );
    assert_eq!(
        rotated(&square, 90.0, BILINEAR).resolution,
        Resolution { x: PPI.y, y: PPI.x },
        "while 90 transposes it, which is what makes the assertion mean something"
    );
    // The growing case's chrome is `padded_*` too: every guide shifts by the
    // origin and none can leave a canvas that only grew.
    let grown = rotated(&square, ANGLE, BILINEAR);
    let origin = rotated_canvas(W, H, ANGLE).expect("the plan").origin();
    assert_eq!(origin, (8, 13));
    assert_eq!(
        guide_pairs(&grown.guides),
        vec![
            (HORIZONTAL, 14.0),
            (HORIZONTAL, 21.25),
            (VERTICAL, 10.0),
            (VERTICAL, 58.5),
        ],
        "hand-written: every guide shifted by the origin, none dropped"
    );
    assert_eq!(grown.ruler_origin, (15.0, 18.0));
}

// --------------------------------------------- (h) what rides unchanged --

#[test]
fn ids_locks_labels_styles_and_linear_light_ride_and_comps_follow_the_rotation() {
    let doc = fixture(W, H);
    let out = rotated(&doc, ANGLE, BILINEAR);

    let ids: Vec<u64> = doc.layers.iter().map(|l| l.id).collect();
    assert_eq!(
        out.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
        ids,
        "the same picture through an op keeps every layer identity"
    );
    assert_eq!(
        out.layers
            .iter()
            .map(|l| l.name.clone())
            .collect::<Vec<_>>(),
        doc.layers
            .iter()
            .map(|l| l.name.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        out.layers.iter().map(|l| l.kind).collect::<Vec<_>>(),
        doc.layers.iter().map(|l| l.kind).collect::<Vec<_>>()
    );

    // LOCK-EXEMPT: the Lock All layer rotated with the picture, exactly as it
    // straightens today.
    let locked = named(&doc, "Locked");
    assert_eq!(doc.layers[locked].locks, LOCK_ALL);
    let locked_out = named(&out, "Locked");
    assert_eq!(out.layers[locked_out].locks, LOCK_ALL, "the flags ride");
    assert_ne!(
        out.layers[locked_out].pixels.dimensions(),
        doc.layers[locked].pixels.dimensions(),
        "and the layer really did rotate"
    );

    // The style factor is sqrt(|det|) = 1, so `scaled_style` hands back the
    // SAME Arc rather than a rebuilt copy.
    let styled = named(&doc, "Styled");
    let styled_out = named(&out, "Styled");
    let before = doc.layers[styled].style.clone().expect("a style");
    let after = out.layers[styled_out].style.clone().expect("a style");
    assert!(
        Arc::ptr_eq(&before, &after),
        "a rotation's scale factor is exactly 1, so the style Arc is shared"
    );

    // The adjustment layer's meta is an OP, not a description of pixels, so
    // the core keeps it.
    let tone = named(&out, "Tone");
    assert_eq!(out.layers[tone].meta, doc.layers[named(&doc, "Tone")].meta);

    // Comps FOLLOW the rotation: the comp was saved with every layer
    // where it sits, so each bound raster entry's recorded offset must now
    // be exactly its layer's post-rotation offset. The Tone adjustment is
    // not moved by the straighten and neither is its entry; a group entry
    // records no offset. Every other stored field — names, ids,
    // visibility, opacity, blend and the style (factor 1) — is identical.
    assert_eq!(out.layer_comps().len(), 1);
    let (before, after) = (&doc.layer_comps()[0], &out.layer_comps()[0]);
    assert_eq!(before.entries.len(), after.entries.len());
    let tone_id = doc.layers[named(&doc, "Tone")].id;
    for (old, new) in before.entries.iter().zip(&after.entries) {
        assert_eq!(new.layer_id, old.layer_id);
        assert_eq!(new.layer_name, old.layer_name);
        assert_eq!(new.visible, old.visible);
        assert_eq!(new.appearance, old.appearance, "{}", old.layer_name);
        let layer = out
            .layers
            .iter()
            .find(|l| l.id == new.layer_id)
            .expect("a bound entry");
        match old.offset {
            Some(_) => assert_eq!(
                new.offset,
                Some(layer.offset),
                "{}'s entry follows its layer",
                old.layer_name
            ),
            None => assert_eq!(new.offset, None, "{}", old.layer_name),
        }
        if new.layer_id == tone_id {
            assert_eq!(new, old, "the Tone entry is untouched");
        }
    }
    assert_eq!(
        (&after.name, &after.comment, after.captures),
        (&before.name, &before.comment, before.captures)
    );

    // 11E (S6): a `linear_light` document rotates and stays linear light. It
    // is built WITHOUT the styled layer, because linear-light evaluation
    // refuses a layer style outright — the claim here is about the storage
    // edit riding the rotation, not about styles.
    let plain = DrDocument::from_pixels(image(W, H, 7));
    let linear = plain
        .with_evaluation(doctored_core::doc_evaluation::Evaluation::LinearLight)
        .expect("the switch is admitted")
        .expect("it is a change");
    assert_eq!(
        rotated(&linear, ANGLE, BILINEAR).evaluation(),
        doctored_core::doc_evaluation::Evaluation::LinearLight
    );
    // The metadata and the profile ride too.
    assert!(Arc::ptr_eq(&out.profile, &doc.profile));
}

// --------------------------------------------------------- (i) refusals --

#[test]
fn every_refusal_states_its_own_sentence_and_identity_is_a_successful_no_op() {
    let doc = fixture(W, H);

    // IDENTITY: NULL through the FFI with NO error at all.
    for degrees in [0.0, -0.0, 360.0, -720.0] {
        let (handle, err) = ffi_rotate_canvas(&doc, degrees, "bilinear");
        assert!(handle.is_null(), "{degrees} must answer NULL");
        assert!(err.is_null(), "{degrees} must answer NO error message");
    }

    // NON-FINITE. JSON has no non-finite number (`json!` writes each as
    // `null`), so the request parser refuses before any angle is read.
    for degrees in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let message = refusal(&doc, degrees, BILINEAR);
        assert!(
            message.starts_with("invalid typed geometry request:"),
            "{degrees}: {message}"
        );
    }

    // A retained source rerenders from its immutable master while the canvas
    // expands; its original asset and identity remain bound.
    let master = DrDocument::from_pixels(image(6, 4, 70));
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"rotate-master","revision_id":"v1",
                "name":"Retained","x":2,"y":1})
        .to_string(),
    )
    .expect("a placement request");
    let placed = doc
        .source_edit(Some(&master), &place, REFERENCE_WORKING_BYTES, 0, None)
        .expect("the placement runs")
        .expect("it is a change");
    assert!(placed.has_retained_source(placed.layers.len() - 1));
    let rotated = rotated(&placed, ANGLE, BILINEAR);
    let index = rotated.layers.len() - 1;
    assert!(rotated.has_retained_source(index));
    assert!(Arc::ptr_eq(
        &placed.layers[index].retained_source.as_ref().unwrap().asset,
        &rotated.layers[index]
            .retained_source
            .as_ref()
            .unwrap()
            .asset,
    ));
    assert_eq!(
        rotated.source_info(index).unwrap()["source_id"],
        "rotate-master"
    );

    // The CANVAS past the 100 MP budget. 14100 x 100 at 45 degrees is
    // 10042 x 10042, which `bounds_of_corners` refuses; the request's plan
    // refuses the same `rotated_canvas` first.
    let wide = DrDocument::from_pixels(RgbaImage::from_pixel(14100, 100, Rgba([9, 9, 9, 255])));
    assert_eq!(
        refusal(&wide, 45.0, BILINEAR),
        "the rotated canvas exceeds supported bounds"
    );

    // 120 channels fit a 4000 x 20 canvas and do not fit the 2844 x 2844 one
    // the rotation would make. The request's plan names the saved-channel
    // budget before it charges those channels against the working limit,
    // so the budget sentence is what the host sees — from the plan export
    // too — and an exact quarter turn is never charged.
    let mut narrow = DrDocument::from_pixels(RgbaImage::from_pixel(4000, 20, Rgba([1, 2, 3, 255])));
    let plane = vec![7u8; 4000 * 20];
    for i in 0..120 {
        narrow = narrow
            .add_channel(&format!("Alpha {i}"), &plane, 4000, 20, [255, 0, 0], 0.5)
            .expect("a channel");
    }
    let budget = "the rotated canvas would exceed the saved-channel pixel budget";
    assert_eq!(refusal(&narrow, 45.0, BILINEAR), budget);
    let request = std::ffi::CString::new(
        json!({"op": "rotate_canvas", "degrees": 45, "filter": "bilinear"}).to_string(),
    )
    .unwrap();
    let mut err: *mut std::ffi::c_char = std::ptr::null_mut();
    let plan = unsafe { dr_doc_geometry_edit_plan(&narrow, request.as_ptr(), &mut err) };
    assert!(plan.is_null(), "the plan must refuse the channel budget");
    assert_eq!(take(err), budget);
    self::rotated(&narrow, 90.0, BILINEAR);

    // A LAYER whose rotated extent is past the pixel limit, on a canvas that
    // is perfectly fine. The request's plan maps each destination first
    // (`mapped_bounds`), so its sentence is what the host sees.
    let mut oversized = DrDocument::from_pixels(image(W, H, 3));
    oversized = oversized
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(14100, 100, Rgba([4, 5, 6, 255])),
            "Huge",
        )
        .expect("the oversized layer");
    assert_eq!(
        refusal(&oversized, 45.0, BILINEAR),
        "byte geometry destination exceeds supported bounds"
    );

    // And the FFI reports a refusal through err_out: an unknown filter name
    // is refused by the request parser.
    let (handle, err) = ffi_rotate_canvas(&doc, 30.0, "gaussian");
    assert!(handle.is_null());
    assert!(!err.is_null(), "a refusal must carry its sentence");
    unsafe { doctored_core::ffi::dr_string_free(err) };

    // NULL document through the FFI.
    let (handle, err) = ffi_rotate_canvas(std::ptr::null(), 30.0, "bilinear");
    assert!(handle.is_null());
    assert!(!err.is_null());
    unsafe { doctored_core::ffi::dr_string_free(err) };
}

// ------------------------------- (j) persistence, purity and the FFI plan --

#[test]
fn native_save_reopen_and_resave_is_byte_identical_and_the_input_is_untouched() {
    let doc = fixture(W, H);
    let before = placement(&doc);
    let before_channel = doc.channels[0].data.as_raw().clone();
    let out = rotated(&doc, ANGLE, BICUBIC);

    // PURITY: the input document is untouched, which is what makes the
    // host's undo stack one handle per step.
    assert_eq!(placement(&doc), before);
    assert_eq!(doc.channels[0].data.as_raw(), &before_channel);
    assert_eq!((doc.width, doc.height), (W, H));

    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("rotated.rz");
    let name = path.to_string_lossy().into_owned();
    out.save_native(&name).expect("native bytes");
    let bytes = std::fs::read(&name).expect("the saved file");
    let reopened = DrDocument::open(&name).expect("reopen");
    assert_eq!(
        (reopened.width, reopened.height),
        (out.width, out.height),
        "the expanded canvas survives"
    );
    assert_eq!(placement(&reopened), placement(&out), "extents survive");
    assert_eq!(
        reopened.channels[0].data.as_raw(),
        out.channels[0].data.as_raw(),
        "the resampled channel survives"
    );
    assert_eq!(guide_pairs(&reopened.guides), guide_pairs(&out.guides));
    assert_eq!(reopened.ruler_origin, out.ruler_origin);
    assert_eq!(reopened.resolution, out.resolution);

    let again = dir.path().join("again.rz");
    let again_name = again.to_string_lossy().into_owned();
    reopened.save_native(&again_name).expect("resave");
    assert_eq!(
        std::fs::read(&again_name).expect("the resaved file"),
        bytes,
        "reopen then resave is byte-identical"
    );
}

#[test]
fn the_plan_export_is_the_one_place_the_host_reads_the_rule() {
    // IDENTITY and 180 keep the canvas; the quarter turns swap it; a
    // filtered angle is R2's box; a refused box is all zeros.
    let cases: [(u32, u32, f64, i32, [i32; 4]); 7] = [
        (64, 48, 0.0, 1, [0, 0, 64, 48]),
        (64, 48, 90.0, 2, [0, 0, 48, 64]),
        (64, 48, 180.0, 3, [0, 0, 64, 48]),
        (64, 48, -90.0, 4, [0, 0, 48, 64]),
        (64, 48, 30.0, 5, [-8, -13, 80, 74]),
        (100, 10, 80.0, 5, [36, -45, 28, 101]),
        (30000, 3000, 45.0, 5, [0, 0, 0, 0]),
    ];
    for (width, height, degrees, code, rect) in cases {
        let mut out_degrees = f64::NAN;
        let mut out_rect = [-1i32; 4];
        let got = unsafe {
            dr_canvas_rotation_plan(
                width,
                height,
                degrees,
                &mut out_degrees,
                out_rect.as_mut_ptr(),
            )
        };
        assert_eq!(got, code, "{width}x{height} @ {degrees}: the code");
        assert_eq!(out_rect, rect, "{width}x{height} @ {degrees}: the rect");
        assert!(out_degrees.is_finite());
    }
    // A non-finite angle is REFUSED, writes zeros into the rect and leaves
    // the degrees alone.
    let mut out_degrees = 12.5f64;
    let mut out_rect = [-1i32; 4];
    assert_eq!(
        unsafe {
            dr_canvas_rotation_plan(64, 48, f64::NAN, &mut out_degrees, out_rect.as_mut_ptr())
        },
        0
    );
    assert_eq!(out_degrees, 12.5);
    assert_eq!(out_rect, [0, 0, 0, 0]);
    // The normalized angle is what comes back.
    let mut out_degrees = 0.0f64;
    assert_eq!(
        unsafe { dr_canvas_rotation_plan(64, 48, 450.0, &mut out_degrees, std::ptr::null_mut()) },
        2
    );
    assert_eq!(out_degrees, 90.0);
    let mut out_degrees = 0.0f64;
    assert_eq!(
        unsafe { dr_canvas_rotation_plan(64, 48, 187.5, &mut out_degrees, std::ptr::null_mut()) },
        5
    );
    assert!((out_degrees - (-172.5)).abs() < 1e-9);
    // Both out pointers may be NULL.
    assert_eq!(
        unsafe {
            dr_canvas_rotation_plan(64, 48, 30.0, std::ptr::null_mut(), std::ptr::null_mut())
        },
        5
    );
}

// ------------------------------------------------ (k) the SIGN of a turn --

#[test]
fn a_positive_angle_turns_the_picture_clockwise() {
    // One opaque marker at the TOP CENTRE of a transparent canvas. Turned
    // CLOCKWISE it must move into the RIGHT half; counter-clockwise, the
    // left half. Nearest, so the marker survives as exactly one sample.
    let mut pixels = RgbaImage::from_pixel(41, 41, Rgba([0, 0, 0, 0]));
    pixels.put_pixel(20, 2, Rgba([255, 0, 0, 255]));
    let doc = DrDocument::from_pixels(pixels);

    for (degrees, expect_right) in [(30.0, true), (-30.0, false)] {
        let out = rotated(&doc, degrees, NEAREST);
        let layer = &out.layers[0];
        let (lw, lh) = layer.pixels.dimensions();
        let mut found: Option<(f64, f64)> = None;
        let image = layer.pixels.contiguous_image();
        for y in 0..lh {
            for x in 0..lw {
                if image.get_pixel(x, y).0[3] > 0 {
                    // Canvas coordinates of the marker's centre.
                    found = Some((
                        f64::from(layer.offset.0) + f64::from(x) + 0.5,
                        f64::from(layer.offset.1) + f64::from(y) + 0.5,
                    ));
                }
            }
        }
        let (mx, my) = found.expect("the marker survived the turn");
        let centre_x = f64::from(out.width) / 2.0;
        let centre_y = f64::from(out.height) / 2.0;
        if expect_right {
            assert!(
                mx > centre_x,
                "+{degrees} must carry the top marker to the RIGHT half: {mx} vs {centre_x}"
            );
        } else {
            assert!(
                mx < centre_x,
                "{degrees} must carry the top marker to the LEFT half: {mx} vs {centre_x}"
            );
        }
        assert!(
            my < centre_y,
            "{degrees}: a 30 degree turn keeps the marker above the centre"
        );
    }
}
