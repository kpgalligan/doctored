//! The typed layer hit test, `dr_precision_doc_layer_at` — the twin of
//! `dr_doc_layer_at` at 16-bit and 32-bit float. Black-box through the FFI.
//! The 16-bit agreement is the byte answer on the same fixture promoted by
//! `dr_doc_promote_structure`; every other expectation is written HERE from
//! the stated rule (alpha clamped to [0, 1] times the enabled mask's
//! coverage, a hit at >= 0.5), never computed by the code under test.

use std::collections::BTreeSet;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, Layer, LayerKind};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_group::dr_doc_layer_at;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_inspect::dr_precision_doc_layer_at;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::tiff_float::FloatTiff;
use image::{GrayImage, Luma, Rgba, RgbaImage};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 40;
const H: u32 = 30;

// ---------------------------------------------------------------- helpers --

fn hit(doc: *const PrecisionDocument, x: i32, y: i32, top_level: bool) -> Option<usize> {
    let mut idx = usize::MAX;
    unsafe { dr_precision_doc_layer_at(doc, x, y, top_level, &mut idx) }.then_some(idx)
}

fn byte_hit(doc: &DrDocument, x: i32, y: i32, top_level: bool) -> Option<usize> {
    let mut idx = usize::MAX;
    unsafe { dr_doc_layer_at(doc, x, y, top_level, &mut idx) }.then_some(idx)
}

fn image16(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u16; 4]) -> PrecisionImage {
    let samples = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| pixel(x, y))
        .collect();
    PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap()
}

/// Every pixel at alpha code `a`.
fn uniform16(w: u32, h: u32, a: u16) -> PrecisionImage {
    image16(w, h, |_, _| [1000, 2000, 3000, a])
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

/// A mask plane `size` at canvas `at`, with its properties.
struct MaskSpec {
    samples: Vec<u16>,
    size: (u32, u32),
    at: (i32, i32),
    outside: u8,
    density: f32,
    feather: f32,
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
        }
    }
    fn mask(&self) -> PrecisionMask {
        let mut p = MaskProperties::at(self.at);
        p.outside_coverage = self.outside;
        p.density = self.density;
        p.feather = self.feather;
        PrecisionMask::new(
            PrecisionPlane::from_u16(self.size.0, self.size.1, self.samples.clone(), None).unwrap(),
            p,
            true,
        )
        .unwrap()
    }
}

// ------------------------------------------------ byte / typed agreement --

/// Alpha codes that sweep the whole 0…255 range across the plane, so the
/// 128 contour runs through every layer.
fn noise(w: u32, h: u32, seed: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let a = ((x * 37 + y * 91 + seed * 53) % 256) as u8;
        Rgba([(x * 13 % 256) as u8, (y * 17 % 256) as u8, 9, a])
    })
}

fn alpha_row(alphas: &[u8]) -> RgbaImage {
    RgbaImage::from_fn(alphas.len() as u32, 1, |x, _| {
        Rgba([10, 20, 30, alphas[x as usize]])
    })
}

fn opaque(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([50, 60, 70, 255]))
}

fn fresh(image: RgbaImage, name: &str) -> Layer {
    let mut layer = DrDocument::from_pixels(image).layers.remove(0);
    layer.name = name.into();
    layer
}

fn at(mut layer: Layer, offset: (i32, i32)) -> Layer {
    layer.offset = offset;
    layer
}

fn with_mask(mut layer: Layer, mask: GrayImage, at: (i32, i32), outside: u8) -> Layer {
    layer.mask = Some(Arc::new(mask));
    layer.mask_properties = MaskProperties::at(at);
    layer.mask_properties.outside_coverage = outside;
    layer.mask_enabled = true;
    layer
}

fn mask_row(codes: &[u8]) -> GrayImage {
    GrayImage::from_fn(codes.len() as u32, 1, |x, _| Luma([codes[x as usize]]))
}

fn mask_noise(w: u32, h: u32, seed: u32) -> GrayImage {
    GrayImage::from_fn(w, h, |x, y| {
        Luma([((x * 71 + y * 29 + seed * 17) % 256) as u8])
    })
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

fn hidden(mut layer: Layer) -> Layer {
    layer.visible = false;
    layer
}

/// Bottom-first:
///  0 Background (noise), 1 Hanging (off the top-left), 2 Beyond (off the
///  bottom-right), 3 Masked (offset noise mask, outside black), 4 White
///  (opaque, offset noise mask, outside white), 5 Disabled (opaque, a zero
///  mask disabled), 6 Child A (depth 1), 7 Pass Child (depth 2), 8 Inner
///  (Screen group, depth 1), 9 Isolated (group whose zero mask hides its
///  children), 10 Hidden (opaque, hidden), 11 Hidden Child (depth 1), 12
///  Hidden Group, 13 Levels (an opaque canvas-sized adjustment), 14 Edge A
///  (alpha 255 under mask 128 / 127), 15 Edge B (alpha 181 / 180 under mask 181 /
///  180), 16 Edge C (alpha 128 / 127, no mask).
fn byte_fixture() -> DrDocument {
    let mut doc = DrDocument::from_pixels(noise(W, H, 1));
    let hanging = at(fresh(noise(14, 10, 2), "Hanging"), (-6, -4));
    let beyond = at(fresh(noise(16, 12, 3), "Beyond"), (30, 24));
    let masked = with_mask(
        at(fresh(noise(17, 11, 4), "Masked"), (5, 4)),
        mask_noise(9, 7, 1),
        (8, 6),
        0,
    );
    let white = with_mask(
        at(fresh(opaque(12, 10), "White"), (20, 2)),
        mask_noise(4, 4, 2),
        (22, 4),
        255,
    );
    let mut disabled = with_mask(
        at(fresh(opaque(10, 8), "Disabled"), (1, 18)),
        GrayImage::new(10, 8),
        (1, 18),
        0,
    );
    disabled.mask_enabled = false;
    let child_a = nested(at(fresh(noise(10, 8, 5), "Child A"), (12, 14)), 1);
    let pass_child = nested(at(fresh(noise(8, 8, 6), "Pass Child"), (24, 12)), 2);
    let inner = nested(group("Inner", BlendMode::Screen), 1);
    let isolated = with_mask(
        group("Isolated", BlendMode::Normal),
        GrayImage::new(W, H),
        (0, 0),
        0,
    );
    let hidden_layer = hidden(fresh(opaque(15, 15), "Hidden"));
    let hidden_child = nested(at(fresh(opaque(10, 10), "Hidden Child"), (28, 0)), 1);
    let hidden_group = hidden(group("Hidden Group", BlendMode::Normal));
    let mut levels = fresh(opaque(W, H), "Levels");
    levels.meta = Some(r#"{"type":"adjust","op":"levels","params":{"gamma":0.8}}"#.into());
    let edge_a = with_mask(
        at(fresh(alpha_row(&[255, 255]), "Edge A"), (36, 0)),
        mask_row(&[128, 127]),
        (36, 0),
        0,
    );
    let edge_b = with_mask(
        at(fresh(alpha_row(&[181, 180]), "Edge B"), (36, 2)),
        mask_row(&[181, 180]),
        (36, 2),
        0,
    );
    let edge_c = at(fresh(alpha_row(&[128, 127]), "Edge C"), (36, 4));
    doc.layers.extend([
        hanging,
        beyond,
        masked,
        white,
        disabled,
        child_a,
        pass_child,
        inner,
        isolated,
        hidden_layer,
        hidden_child,
        hidden_group,
        levels,
        edge_a,
        edge_b,
        edge_c,
    ]);
    doc
}

#[test]
fn a_promoted_16_bit_document_hits_what_the_byte_document_hits() {
    let byte = byte_fixture();
    let mut error = ptr::null_mut();
    let typed = unsafe { dr_doc_promote_structure(&byte, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!typed.is_null(), "promotion failed");
    assert_eq!(unsafe { &*typed }.layer_count(), byte.layers.len());

    let mut leaves = BTreeSet::new();
    let mut tops = BTreeSet::new();
    for top_level in [false, true] {
        for y in -4..H as i32 + 4 {
            for x in -4..W as i32 + 4 {
                let expected = byte_hit(&byte, x, y, top_level);
                let got = hit(typed, x, y, top_level);
                assert_eq!(got, expected, "({x}, {y}) top_level {top_level}");
                if let Some(idx) = got {
                    if top_level {
                        tops.insert(idx);
                    } else {
                        leaves.insert(idx);
                    }
                }
            }
        }
    }
    // The fixture exercises real answers: every visible raster outside the
    // hidden subtrees is hit somewhere, the group answers at top level, and
    // nothing hidden or adjusting ever is.
    assert_eq!(
        leaves,
        BTreeSet::from([0, 1, 2, 3, 4, 5, 6, 7, 14, 15, 16]),
        "leaf answers"
    );
    assert_eq!(
        tops,
        BTreeSet::from([0, 1, 2, 3, 4, 5, 9, 14, 15, 16]),
        "top-level answers"
    );

    // The edge codes, stated: round(255·128/255) = 128 hits, 127 misses;
    // round(181·181/255) = 128 hits, round(180·180/255) = 127 misses; a bare
    // alpha 128 hits, 127 misses.
    assert_eq!(hit(typed, 36, 0, false), Some(14));
    assert_ne!(hit(typed, 37, 0, false), Some(14));
    assert_eq!(hit(typed, 36, 2, false), Some(15));
    assert_ne!(hit(typed, 37, 2, false), Some(15));
    assert_eq!(hit(typed, 36, 4, false), Some(16));
    assert_ne!(hit(typed, 37, 4, false), Some(16));
    // Opaque outside a white-outside mask hits; the disabled zero mask is
    // ignored; outside a black-outside mask misses.
    assert_eq!(hit(typed, 21, 3, false), Some(4));
    assert_eq!(hit(typed, 2, 19, false), Some(5));
    assert_ne!(hit(typed, 6, 5, false), Some(3));
    // Off the canvas, only the layers hanging there can answer.
    assert!(matches!(hit(typed, -5, -3, false), None | Some(1)));
    assert!(matches!(hit(typed, 44, 33, false), None | Some(2)));
    unsafe { dr_precision_doc_free(typed) };
}

// ------------------------------------------------------- the stated rule --

#[test]
fn alpha_alone_hits_at_one_half() {
    // 32768 / 65535 > 0.5 hits; 32767 / 65535 < 0.5 misses.
    let doc = document(vec![
        raster("Over", uniform16(1, 1, 32768), (3, 3)),
        raster("Under", uniform16(1, 1, 32767), (5, 3)),
    ]);
    assert_eq!(hit(&doc, 3, 3, false), Some(0));
    assert_eq!(hit(&doc, 5, 3, false), None);
    assert_eq!(hit(&doc, 4, 3, false), None, "between the two");
    assert_eq!(hit(&doc, -1, 3, false), None, "off the canvas");
}

#[test]
fn density_lifts_a_zero_mask_by_its_complement() {
    // Coverage 1 − d·(1 − 0): density 0.49 leaves 0.51 (hit), 0.51 leaves
    // 0.49 (miss), under an opaque layer.
    for (density, expected) in [(0.49, Some(0)), (0.51, None)] {
        let mut spec = MaskSpec::new((4, 4), (0, 0), |_, _| 0);
        spec.density = density;
        let doc = document(vec![
            raster("D", uniform16(4, 4, 65535), (0, 0)).with_mask(Some(spec.mask()))
        ]);
        assert_eq!(hit(&doc, 1, 1, false), expected, "density {density}");
    }
}

#[test]
fn a_feathered_hard_edged_mask_hits_deep_inside_and_misses_deep_outside() {
    let mut spec = MaskSpec::new((10, 10), (15, 10), |_, _| 65535);
    spec.feather = 3.0;
    let doc = document(vec![
        raster("F", uniform16(W, H, 65535), (0, 0)).with_mask(Some(spec.mask()))
    ]);
    assert_eq!(hit(&doc, 20, 15, false), Some(0), "deep inside");
    assert_eq!(hit(&doc, 2, 2, false), None, "deep outside");
    assert_eq!(hit(&doc, 38, 28, false), None, "deep outside");
}

#[test]
fn an_unpreparable_feather_falls_back_to_the_alpha_test() {
    // A zero mask with the maximum feather over a plane large enough that
    // the blur's planned work exceeds the interactive budget
    // (`MAX_WORK_UNITS`): the hit test has no job control, so it does not
    // prepare the feather (`prepare` itself no longer refuses) and
    // the mask counts as none. Were it applied, a zero plane over outside
    // coverage 0 would hit nothing.
    let image = image16(12, 10, |x, y| {
        [0, 0, 0, if x >= 1 && y >= 2 { 65535 } else { 0 }]
    });
    let (pw, ph) = (6400u32, 6400u32);
    let mut properties = MaskProperties::at((0, 0));
    properties.feather = 1000.0;
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(pw, ph, vec![0; (pw * ph) as usize], None).unwrap(),
        properties,
        true,
    )
    .unwrap();
    let doc = document(vec![raster("Big", image, (4, 4)).with_mask(Some(mask))]);
    assert_eq!(hit(&doc, 9, 9, false), Some(0), "opaque: the alpha test");
    assert_eq!(hit(&doc, 4, 4, false), None, "transparent: still a miss");
}

#[test]
fn a_typed_group_answers_its_leaf_or_itself_at_top_level() {
    let inner = PrecisionDocumentLayer::pass_through(
        "Inner",
        vec![raster("B", uniform16(4, 4, 65535), (10, 10))],
        1.0,
        true,
    )
    .unwrap();
    let outer = PrecisionDocumentLayer::isolated_normal(
        "Outer",
        vec![raster("A", uniform16(4, 4, 65535), (0, 0)), inner],
        1.0,
        true,
    )
    .unwrap()
    .with_mask(Some(MaskSpec::new((W, H), (0, 0), |_, _| 0).mask()));
    let hidden_group = PrecisionDocumentLayer::isolated_normal(
        "Hidden",
        vec![raster("C", uniform16(W, H, 65535), (0, 0))],
        1.0,
        false,
    )
    .unwrap();
    let doc = document(vec![
        raster("Bottom", uniform16(2, 2, 65535), (20, 20)),
        outer,
        hidden_group,
    ]);
    // Postorder: 0 Bottom, 1 A, 2 B, 3 Inner, 4 Outer, 5 C, 6 Hidden.
    assert_eq!(doc.layer_count(), 7);
    assert_eq!(hit(&doc, 1, 1, false), Some(1));
    assert_eq!(hit(&doc, 1, 1, true), Some(4));
    assert_eq!(hit(&doc, 11, 11, false), Some(2));
    assert_eq!(hit(&doc, 11, 11, true), Some(4));
    assert_eq!(hit(&doc, 20, 20, false), Some(0));
    assert_eq!(
        hit(&doc, 20, 20, true),
        Some(0),
        "a root leaf is its own top"
    );
    assert_eq!(
        hit(&doc, 30, 5, false),
        None,
        "only the hidden group covers"
    );
}

// ---------------------------------------------------------------- float --

/// A float document holding one raster of `samples` (RGBA, row-major),
/// through a float TIFF opened with `open_preserving` — the only way a
/// float raster is made, so import's repair (NaN alpha to 0, alpha above 1
/// to 1) runs before the hit test ever sees a sample.
fn float_document(width: u32, height: u32, samples: Vec<f32>) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples,
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("hit.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// 6×1: alpha 0.5, 0.4999, 2.0, NaN, 1.0, 0.0, with above-white RGB.
fn float_row() -> PrecisionDocument {
    let alphas = [0.5, 0.4999, 2.0, f32::NAN, 1.0, 0.0];
    let samples = alphas.iter().flat_map(|&a| [1.75, -0.5, 0.25, a]).collect();
    float_document(6, 1, samples)
}

#[test]
fn float_alpha_hits_at_one_half_and_a_mask_scales_it() {
    let doc = float_row();
    assert_eq!(hit(&doc, 0, 0, false), Some(0), "alpha 0.5");
    assert_eq!(hit(&doc, 1, 0, false), None, "alpha 0.4999");
    assert_eq!(hit(&doc, 2, 0, false), Some(0), "alpha 2.0, unmasked");
    assert_eq!(hit(&doc, 3, 0, false), None, "NaN alpha");
    assert_eq!(hit(&doc, 5, 0, false), None, "alpha 0");
    // Coverage 0.4 (26214 / 65535) at the alpha-2.0 pixel, 1 elsewhere: an
    // alpha clamped to 1 gives 0.4 < 0.5, a miss (unclamped it would be 0.8).
    let mask = MaskSpec::new((6, 1), (0, 0), |x, _| if x == 2 { 26214 } else { 65535 });
    let masked = doc
        .set_layer_mask(0, Some(mask.mask()), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        hit(&masked, 2, 0, false),
        None,
        "alpha 2.0 under coverage 0.4"
    );
    assert_eq!(
        hit(&masked, 4, 0, false),
        Some(0),
        "alpha 1.0 under coverage 1"
    );
}

#[test]
fn a_float_group_answers_itself_at_top_level() {
    let doc = float_row();
    let copied = doc.duplicate_layer(0).unwrap().unwrap();
    // The copy hides its pixel 4 under a zero mask, so the original shows.
    let mask = MaskSpec::new((6, 1), (0, 0), |x, _| if x == 4 { 0 } else { 65535 });
    let masked = copied
        .set_layer_mask(1, Some(mask.mask()), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let grouped = masked
        .group_layers(&[1], "Group", false)
        .unwrap()
        .unwrap()
        .0;
    // Postorder: 0 original, 1 copy, 2 Group.
    assert_eq!(grouped.layer_count(), 3);
    assert_eq!(hit(&grouped, 0, 0, false), Some(1));
    assert_eq!(hit(&grouped, 0, 0, true), Some(2));
    assert_eq!(hit(&grouped, 4, 0, false), Some(0));
    assert_eq!(hit(&grouped, 4, 0, true), Some(0));
    assert_eq!(hit(&grouped, 1, 0, true), None);
}

// ------------------------------------------------------------ NULL safety --

#[test]
fn null_and_miss_answers_write_nothing() {
    let doc = document(vec![raster("L", uniform16(4, 4, 65535), (0, 0))]);
    let mut idx = 777usize;
    assert!(!unsafe { dr_precision_doc_layer_at(ptr::null(), 0, 0, false, &mut idx) });
    assert_eq!(idx, 777, "a NULL document writes nothing");
    assert!(unsafe { dr_precision_doc_layer_at(&doc, 1, 1, false, ptr::null_mut()) });
    assert!(unsafe { dr_precision_doc_layer_at(&doc, 1, 1, true, ptr::null_mut()) });
    assert!(!unsafe { dr_precision_doc_layer_at(&doc, 10, 10, false, &mut idx) });
    assert_eq!(idx, 777, "a miss writes nothing");
    assert!(!unsafe { dr_precision_doc_layer_at(&doc, i32::MIN, i32::MAX, true, &mut idx) });
    assert_eq!(idx, 777);
    assert!(unsafe { dr_precision_doc_layer_at(&doc, 0, 0, false, &mut idx) });
    assert_eq!(idx, 0);
}
