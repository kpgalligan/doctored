//! The typed content box, `dr_precision_doc_layer_bounds` — the twin
//! of `dr_doc_layer_bounds` at 16-bit and 32-bit float. Black-box through
//! the FFI. Every expected box is a brute-force loop over every pixel written
//! HERE (alpha > 0 and an independently restated mask coverage > 0), never
//! `edge_bounds` compared with itself; the byte/typed agreement is the byte
//! export on the same fixture promoted by `dr_doc_promote_structure`.

use crate::serial;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, Layer, LayerKind};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_group::dr_doc_layer_bounds;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_inspect::dr_precision_doc_layer_bounds;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::tiff_float::FloatTiff;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::json;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 40;
const H: u32 = 30;

type Rect = (i32, i32, i32, i32);

// ---------------------------------------------------------------- helpers --

fn bounds(doc: &PrecisionDocument, idx: usize) -> Option<Rect> {
    let mut xywh = [0i32; 4];
    unsafe { dr_precision_doc_layer_bounds(doc, idx, xywh.as_mut_ptr()) }
        .then_some((xywh[0], xywh[1], xywh[2], xywh[3]))
}

fn image16(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u16; 4]) -> PrecisionImage {
    let samples = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| pixel(x, y))
        .collect();
    PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap()
}

/// Opaque where `inside` holds, fully transparent (with nonzero RGB) elsewhere.
fn alpha16(w: u32, h: u32, inside: impl Fn(u32, u32) -> bool) -> PrecisionImage {
    image16(w, h, |x, y| {
        [
            (x * 999) as u16,
            (y * 777) as u16,
            4321,
            if inside(x, y) { 40_000 } else { 0 },
        ]
    })
}

fn raster(name: &str, image: PrecisionImage, offset: (i32, i32)) -> PrecisionDocumentLayer {
    PrecisionDocumentLayer::raster(name, image, offset, 1.0, true).unwrap()
}

fn document(layers: Vec<PrecisionDocumentLayer>) -> PrecisionDocument {
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

/// A mask plane `pw` x `ph` at canvas `at`, with its properties.
struct MaskSpec {
    samples: Vec<u16>,
    size: (u32, u32),
    at: (i32, i32),
    outside: u8,
    density: f32,
    feather: f32,
    enabled: bool,
}

impl MaskSpec {
    fn new(size: (u32, u32), at: (i32, i32), sample: impl Fn(u32, u32) -> u16) -> Self {
        let samples = (0..size.1)
            .flat_map(|y| (0..size.0).map(move |x| (x, y)))
            .map(|(x, y)| sample(x, y))
            .collect();
        Self {
            samples,
            size,
            at,
            outside: 0,
            density: 1.0,
            feather: 0.0,
            enabled: true,
        }
    }
    fn properties(&self) -> MaskProperties {
        let mut p = MaskProperties::at(self.at);
        p.outside_coverage = self.outside;
        p.density = self.density;
        p.feather = self.feather;
        p
    }
    fn mask(&self) -> PrecisionMask {
        PrecisionMask::new(
            PrecisionPlane::from_u16(self.size.0, self.size.1, self.samples.clone(), None).unwrap(),
            self.properties(),
            self.enabled,
        )
        .unwrap()
    }
    /// The effective coverage at canvas `(x, y)`, restated: the plane's
    /// sample inside it, the outside coverage beyond it, then density.
    /// Unfeathered masks only.
    fn coverage(&self, x: i64, y: i64) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        assert_eq!(
            self.feather, 0.0,
            "the oracle restates unfeathered masks only"
        );
        let (px, py) = (x - i64::from(self.at.0), y - i64::from(self.at.1));
        let c = if px >= 0 && py >= 0 && px < i64::from(self.size.0) && py < i64::from(self.size.1)
        {
            f32::from(self.samples[py as usize * self.size.0 as usize + px as usize]) / 65535.0
        } else {
            f32::from(self.outside) / 255.0
        };
        1.0 - self.density * (1.0 - c)
    }
}

/// The brute-force box: every buffer pixel of `image` at `offset` whose
/// alpha is > 0 and whose canvas coverage is > 0.
fn oracle(
    image: &PrecisionImage,
    offset: (i32, i32),
    coverage: impl Fn(i64, i64) -> f32,
) -> Option<Rect> {
    let (w, h) = image.dimensions();
    let mut found: Option<(i64, i64, i64, i64)> = None;
    for y in 0..h {
        for x in 0..w {
            let alpha = image.sample_at(x, y).unwrap()[3];
            let (cx, cy) = (
                i64::from(offset.0) + i64::from(x),
                i64::from(offset.1) + i64::from(y),
            );
            if alpha > 0.0 && coverage(cx, cy) > 0.0 {
                found = Some(match found {
                    None => (cx, cy, cx + 1, cy + 1),
                    Some((a, b, c, d)) => (a.min(cx), b.min(cy), c.max(cx + 1), d.max(cy + 1)),
                });
            }
        }
    }
    found.map(|(x0, y0, x1, y1)| (x0 as i32, y0 as i32, (x1 - x0) as i32, (y1 - y0) as i32))
}

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (None, b) => b,
        (a, None) => a,
        (Some(a), Some(b)) => {
            let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
            let (x1, y1) = ((a.0 + a.2).max(b.0 + b.2), (a.1 + a.3).max(b.1 + b.3));
            Some((x0, y0, x1 - x0, y1 - y0))
        }
    }
}

fn within(inner: Rect, outer: Rect) -> bool {
    inner.0 >= outer.0
        && inner.1 >= outer.1
        && inner.0 + inner.2 <= outer.0 + outer.2
        && inner.1 + inner.3 <= outer.1 + outer.3
}

// ------------------------------------------------------------------ u16 --

#[test]
fn u16_rasters_answer_their_alpha_box_sparse_single_full_empty_and_off_canvas() {
    let _serial = serial();
    let sparse = alpha16(16, 12, |x, y| [(3, 2), (10, 9), (7, 5)].contains(&(x, y)));
    let single = alpha16(9, 7, |x, y| (x, y) == (5, 4));
    let full = alpha16(16, 12, |_, _| true);
    let empty = alpha16(8, 8, |_, _| false);
    let negative = alpha16(10, 8, |x, y| x >= 2 && y >= 1 && x < 9);
    let beyond = alpha16(6, 6, |x, _| x < 4);
    let cases = [
        (sparse, (2, 3)),
        (single, (0, 0)),
        (full, (1, 1)),
        (empty, (4, 4)),
        (negative, (-5, -3)),
        (beyond, (38, 27)),
    ];
    let doc = document(
        cases
            .iter()
            .enumerate()
            .map(|(i, (image, offset))| raster(&format!("L{i}"), image.clone(), *offset))
            .collect(),
    );
    for (i, (image, offset)) in cases.iter().enumerate() {
        assert_eq!(
            bounds(&doc, i),
            oracle(image, *offset, |_, _| 1.0),
            "layer {i}"
        );
    }
    assert_eq!(bounds(&doc, 1), Some((5, 4, 1, 1)));
    assert_eq!(bounds(&doc, 2), Some((1, 1, 16, 12)));
    assert_eq!(bounds(&doc, 3), None, "an empty raster has no content box");
    assert_eq!(
        bounds(&doc, 4),
        Some((-3, -2, 7, 7)),
        "not clipped to the canvas"
    );
    assert_eq!(
        bounds(&doc, 5),
        Some((38, 27, 4, 6)),
        "not clipped to the canvas"
    );
    assert_eq!(bounds(&doc, cases.len()), None, "out of range");
}

#[test]
fn a_null_out_pointer_still_answers() {
    let _serial = serial();
    let doc = document(vec![raster("L", alpha16(4, 4, |_, _| true), (0, 0))]);
    assert!(unsafe { dr_precision_doc_layer_bounds(&doc, 0, ptr::null_mut()) });
    assert!(!unsafe { dr_precision_doc_layer_bounds(&doc, 1, ptr::null_mut()) });
    assert!(!unsafe { dr_precision_doc_layer_bounds(ptr::null(), 0, ptr::null_mut()) });
}

#[test]
fn the_hidden_flag_is_ignored() {
    let _serial = serial();
    let layer =
        PrecisionDocumentLayer::raster("Hidden", alpha16(6, 5, |x, _| x > 2), (3, 3), 1.0, false)
            .unwrap();
    let doc = document(vec![layer]);
    assert_eq!(bounds(&doc, 0), Some((6, 3, 3, 5)));
}

// ---------------------------------------------------------------- masks --

fn masked_case(
    image: PrecisionImage,
    offset: (i32, i32),
    spec: &MaskSpec,
) -> (Option<Rect>, Option<Rect>) {
    let doc = document(vec![
        raster("M", image.clone(), offset).with_mask(Some(spec.mask()))
    ]);
    (
        bounds(&doc, 0),
        oracle(&image, offset, |x, y| spec.coverage(x, y)),
    )
}

#[test]
fn an_enabled_offset_mask_cuts_the_box() {
    let _serial = serial();
    let image = alpha16(16, 12, |_, _| true);
    let spec = MaskSpec::new(
        (6, 5),
        (5, 6),
        |x, y| if x == 0 && y == 0 { 0 } else { 65535 },
    );
    let (got, expected) = masked_case(image, (2, 2), &spec);
    assert_eq!(got, expected);
    assert_eq!(got, Some((5, 6, 6, 5)));
}

#[test]
fn a_disabled_mask_is_ignored() {
    let _serial = serial();
    let image = alpha16(16, 12, |x, _| x >= 3);
    let mut spec = MaskSpec::new((2, 2), (4, 4), |_, _| 65535);
    spec.enabled = false;
    let (got, expected) = masked_case(image, (0, 0), &spec);
    assert_eq!(got, expected);
    assert_eq!(got, Some((3, 0, 13, 12)));
}

#[test]
fn density_half_leaves_every_opaque_pixel_covered() {
    let _serial = serial();
    let image = alpha16(16, 12, |x, y| x >= 2 && y >= 1);
    let mut spec = MaskSpec::new((16, 12), (0, 0), |_, _| 0);
    spec.density = 0.5;
    let (got, expected) = masked_case(image, (0, 0), &spec);
    assert_eq!(got, expected);
    assert_eq!(got, Some((2, 1, 14, 11)));
}

#[test]
fn outside_coverage_white_around_a_mask_smaller_than_the_buffer() {
    let _serial = serial();
    let image = alpha16(16, 12, |_, _| true);
    let mut spec = MaskSpec::new((5, 12), (0, 0), |_, _| 0);
    spec.outside = 255;
    let (got, expected) = masked_case(image, (0, 0), &spec);
    assert_eq!(got, expected);
    assert_eq!(got, Some((5, 0, 11, 12)));
}

#[test]
fn a_feathered_mask_box_lies_between_the_unfeathered_box_and_the_alpha_box() {
    let _serial = serial();
    let image = alpha16(24, 20, |_, _| true);
    let spec = MaskSpec::new((6, 6), (9, 7), |_, _| 65535);
    let sharp = document(vec![
        raster("S", image.clone(), (0, 0)).with_mask(Some(spec.mask()))
    ]);
    let unfeathered = bounds(&sharp, 0).unwrap();
    assert_eq!(unfeathered, (9, 7, 6, 6));
    let mut soft = MaskSpec::new((6, 6), (9, 7), |_, _| 65535);
    soft.feather = 3.0;
    let doc = document(vec![raster("F", image, (0, 0)).with_mask(Some(soft.mask()))]);
    let feathered = bounds(&doc, 0).unwrap();
    assert!(within(feathered, (0, 0, 24, 20)), "{feathered:?}");
    assert!(within(unfeathered, feathered), "{feathered:?}");
    assert_ne!(feathered, unfeathered, "the feather spreads coverage");
}

#[test]
fn an_unpreparable_feather_falls_back_to_the_unmasked_alpha_box() {
    let _serial = serial();
    // A zero mask with the maximum feather over a plane large enough that
    // the blur's planned work exceeds the interactive budget
    // (`MAX_WORK_UNITS`): the scan has no job control, so it does not
    // prepare the feather (`prepare` itself no longer refuses), and
    // the answer is the alpha box. Were the mask applied, a zero plane over
    // outside coverage 0 would leave nothing.
    let image = alpha16(12, 10, |x, y| x >= 1 && y >= 2);
    let (pw, ph) = (6400u32, 6400u32);
    let mut properties = MaskProperties::at((0, 0));
    properties.feather = 1000.0;
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(pw, ph, vec![0; (pw * ph) as usize], None).unwrap(),
        properties,
        true,
    )
    .unwrap();
    let doc = document(vec![
        raster("Big", image.clone(), (4, 4)).with_mask(Some(mask))
    ]);
    assert_eq!(bounds(&doc, 0), Some((5, 6, 11, 8)));
    assert_eq!(bounds(&doc, 0), oracle(&image, (4, 4), |_, _| 1.0));
}

// ----------------------------------------------------------------- memo --

#[test]
fn the_memo_hits_on_a_repeat_and_misses_after_a_mask_edit() {
    let _serial = serial();
    let image = alpha16(16, 12, |_, _| true);
    let spec = MaskSpec::new((5, 12), (0, 0), |_, _| 65535);
    // One mask value: every edit below keeps its plane Arc, so each miss
    // proves the edited property itself is in the key.
    let mask = spec.mask();
    let doc = document(vec![
        raster("Memo", image.clone(), (0, 0)).with_mask(Some(mask.clone()))
    ]);
    let held = doc.layer_source(0).unwrap().clone();
    assert!(PrecisionImage::ptr_eq(&held, &image));
    assert_eq!(held.weak_count(), 0, "nothing measured yet");
    let first = bounds(&doc, 0);
    assert_eq!(first, Some((0, 0, 5, 12)));
    assert_eq!(held.weak_count(), 1, "one memo entry");
    assert_eq!(bounds(&doc, 0), first, "a hit answers the same box");
    assert_eq!(held.weak_count(), 1, "a hit stores nothing new");

    // A density edit on the SAME plane keeps the raster (same identity)
    // and changes coverage.
    let mut half = mask.properties().clone();
    half.density = 0.5;
    let edited = doc
        .set_layer_mask(
            0,
            Some(mask.with_properties(half, true).unwrap()),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(PrecisionImage::ptr_eq(
        edited.layer_source(0).unwrap(),
        &image
    ));
    assert_eq!(
        bounds(&edited, 0),
        Some((0, 0, 16, 12)),
        "the edit invalidates"
    );
    assert_eq!(held.weak_count(), 2, "the edited mask is a second key");
    assert_eq!(bounds(&doc, 0), first, "the original still hits");
    assert_eq!(held.weak_count(), 2);

    // A mask offset edit on the SAME plane is a key too.
    let mut shifted = mask.properties().clone();
    shifted.offset = (4, 0);
    let offset_edit = doc
        .set_layer_mask(
            0,
            Some(mask.with_properties(shifted, true).unwrap()),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(bounds(&offset_edit, 0), Some((4, 0, 5, 12)));
    assert_eq!(held.weak_count(), 3);

    // Moving the record, with the same mask value, is a key too.
    let moved = document(vec![
        raster("Memo", image.clone(), (3, 1)).with_mask(Some(mask.clone()))
    ]);
    assert_eq!(bounds(&moved, 0), Some((3, 1, 2, 11)));
    assert_eq!(held.weak_count(), 4);
}

// --------------------------------------------------------------- groups --

#[test]
fn groups_union_their_raster_descendants_and_ignore_their_own_mask() {
    let _serial = serial();
    let a = alpha16(10, 8, |_, _| true);
    let a_mask = MaskSpec::new((4, 8), (1, 1), |_, _| 65535);
    let b = alpha16(5, 5, |x, y| x + y >= 4);
    let c = alpha16(3, 3, |_, _| true);
    let zero_group_mask = MaskSpec::new((W, H), (0, 0), |_, _| 0);
    let inner = PrecisionDocumentLayer::isolated_normal(
        "Inner",
        vec![raster("B", b.clone(), (20, 15))],
        1.0,
        true,
    )
    .unwrap()
    .with_mask(Some(zero_group_mask.mask()));
    let outer = PrecisionDocumentLayer::pass_through(
        "Outer",
        vec![
            raster("A", a.clone(), (1, 1)).with_mask(Some(a_mask.mask())),
            inner,
        ],
        1.0,
        true,
    )
    .unwrap()
    .with_mask(Some(zero_group_mask.mask()));
    let empty_group = PrecisionDocumentLayer::isolated_normal(
        "Empty",
        vec![raster("Clear", alpha16(4, 4, |_, _| false), (0, 0))],
        1.0,
        true,
    )
    .unwrap();
    let doc = document(vec![outer, raster("C", c.clone(), (-2, 28)), empty_group])
        .add_adjustment(
            "Exposure",
            &json!({"type":"adjust","evaluation":"precision_v1","op":"exposure",
                "params":{"exposure":0.25}})
            .to_string(),
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    // Postorder: 0 A, 1 B, 2 Inner, 3 Outer, 4 C, 5 Clear, 6 Empty, 7 Exposure.
    assert_eq!(doc.layer_count(), 8);
    let a_box = oracle(&a, (1, 1), |x, y| a_mask.coverage(x, y));
    let b_box = oracle(&b, (20, 15), |_, _| 1.0);
    assert_eq!(bounds(&doc, 0), a_box);
    assert_eq!(a_box, Some((1, 1, 4, 8)));
    assert_eq!(bounds(&doc, 1), b_box);
    assert_eq!(
        bounds(&doc, 2),
        b_box,
        "the group's own zero mask is ignored"
    );
    assert_eq!(
        bounds(&doc, 3),
        union(a_box, b_box),
        "nested union through Pass Through"
    );
    assert_eq!(bounds(&doc, 4), oracle(&c, (-2, 28), |_, _| 1.0));
    assert_eq!(bounds(&doc, 5), None);
    assert_eq!(bounds(&doc, 6), None, "a group of nothing opaque");
    assert_eq!(
        bounds(&doc, 7),
        None,
        "an adjustment record has no content box"
    );
    assert_eq!(bounds(&doc, 8), None, "out of range");
}

// ---------------------------------------------------------------- float --

/// A float document authored here: transparent margins, an opaque block
/// holding above-white and negative components, and a half-alpha pixel.
fn float_document() -> PrecisionDocument {
    let (w, h) = (12u32, 10u32);
    let mut samples = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let alpha = if (3..8).contains(&x) && (2..6).contains(&y) {
                1.0
            } else if (x, y) == (9, 8) {
                0.25
            } else {
                0.0
            };
            samples.extend_from_slice(&[1.75, -0.5, 0.25, alpha]);
        }
    }
    let image = FloatTiff {
        width: w,
        height: h,
        channels: 4,
        alpha: Some(false),
        samples,
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&image, false).expect("float TIFF bytes");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("content.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the authored float document opens");
    assert!(doc.is_float());
    doc
}

#[test]
fn a_float_raster_answers_its_alpha_box() {
    let _serial = serial();
    let doc = float_document();
    let image = doc.layer_source(0).unwrap();
    assert_eq!(
        bounds(&doc, 0),
        oracle(image, doc.layer(0).unwrap().offset(), |_, _| 1.0)
    );
    assert_eq!(bounds(&doc, 0), Some((3, 2, 7, 7)));
}

// ------------------------------------------------ byte / typed agreement --

fn pixels(w: u32, h: u32, seed: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let a = if (x * 7 + y * 3 + seed).is_multiple_of(5) || x < seed % 3 {
            0
        } else {
            200
        };
        Rgba([(x * 13 % 256) as u8, (y * 17 % 256) as u8, 9, a])
    })
}

fn fresh(image: RgbaImage, name: &str) -> Layer {
    let mut layer = DrDocument::from_pixels(image).layers.remove(0);
    layer.name = name.into();
    layer
}

fn with_mask(mut layer: Layer, size: (u32, u32), at: (i32, i32), outside: u8, seed: u32) -> Layer {
    layer.mask = Some(Arc::new(GrayImage::from_fn(size.0, size.1, |x, y| {
        Luma([if (x + y + seed).is_multiple_of(4) {
            0
        } else {
            ((x * 31 + y * 11) % 255 + 1) as u8
        }])
    })));
    layer.mask_properties = MaskProperties::at(at);
    layer.mask_properties.outside_coverage = outside;
    layer.mask_enabled = true;
    layer
}

fn group(name: &str, blend: BlendMode) -> Layer {
    let mut layer = fresh(RgbaImage::new(1, 1), name);
    layer.kind = LayerKind::Group;
    layer.blend = blend;
    layer
}

fn nested(mut layer: Layer, depth: u16) -> Layer {
    layer.depth = depth;
    layer
}

fn byte_fixture() -> DrDocument {
    let mut doc = DrDocument::from_pixels(pixels(W, H, 1));
    let mut masked = with_mask(fresh(pixels(17, 11, 2), "Masked"), (9, 7), (8, 6), 0, 1);
    masked.offset = (5, 4);
    let mut child_a = nested(fresh(pixels(10, 8, 4), "Child A"), 1);
    child_a.offset = (-3, 12);
    let mut child_b = nested(
        with_mask(
            fresh(pixels(20, 12, 5), "Child B"),
            (6, 4),
            (26, 20),
            255,
            2,
        ),
        1,
    );
    child_b.offset = (22, 17);
    let isolated = group("Isolated", BlendMode::Normal);
    let mut pass_child = nested(fresh(pixels(25, 20, 6), "Pass Child"), 2);
    pass_child.offset = (30, 25);
    let inner = nested(group("Inner", BlendMode::Screen), 1);
    let pass = group("Pass", BlendMode::PassThrough);
    let clear = fresh(RgbaImage::new(7, 7), "Clear");
    doc.layers.extend([
        masked, child_a, child_b, isolated, pass_child, inner, pass, clear,
    ]);
    doc
}

#[test]
fn typed_boxes_agree_with_byte_boxes_after_promotion() {
    let _serial = serial();
    let byte = byte_fixture();
    let mut error = ptr::null_mut();
    let typed = unsafe { dr_doc_promote_structure(&byte, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!typed.is_null(), "promotion failed");
    let typed_ref = unsafe { &*typed };
    assert_eq!(typed_ref.layer_count(), byte.layers.len());
    let mut some = 0;
    for idx in 0..byte.layers.len() + 1 {
        let mut b = [0i32; 4];
        let byte_answer = unsafe { dr_doc_layer_bounds(&byte, idx, b.as_mut_ptr()) }.then_some(b);
        let mut t = [0i32; 4];
        let typed_answer =
            unsafe { dr_precision_doc_layer_bounds(typed, idx, t.as_mut_ptr()) }.then_some(t);
        assert_eq!(typed_answer, byte_answer, "record {idx}");
        some += usize::from(byte_answer.is_some());
    }
    assert!(some >= 7, "the fixture exercises real boxes");
    unsafe { dr_precision_doc_free(typed) };
}
