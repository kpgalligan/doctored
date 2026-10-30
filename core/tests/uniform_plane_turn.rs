//! A 16-bit raster whose samples are all bitwise equal (File ▸
//! New's white background) resamples with a constant read into a banded,
//! tile-sharing output (`precision_sampling::transform_image`'s uniform
//! branch), and the geometry plan charges that band instead of the source
//! copy and the adoption copy (`precision_geometry_plan`).
//!
//! Oracles: SHA-256 pins of every output sample, captured with the former
//! path (HEAD before the change, the `native_save_bytes.rs` precedent; a
//! pinned digest is never edited); twin documents whose source differs in
//! one corner pixel, which take the former path at HEAD and now; the plane's
//! retained bytes written out here from its own samples on the 256 grid; the
//! plan figures HEAD gave the paths this change must not move.

use std::collections::HashSet;

use doctored_core::doc_mask::MaskProperties;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::retained_source::SourceEdit;
use doctored_core::tiff_float::{encode_to_vec, FloatTiff};
use serde_json::{json, Value};

use crate::common::sha256;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The working limit of the assumed minimum machine, a 16 GB Mac.
const MINIMUM_MAC_LIMIT: u64 = 8 << 30;
const TILE: u32 = 256;
const VALUES: [[u16; 4]; 3] = [[65535; 4], [1000, 30000, 65535, 40000], [5, 6, 7, 0]];
const CANVAS: (u32, u32) = (800, 600);
const RASTER: (u32, u32) = (900, 700);
const OFFSET: (i32, i32) = (-37, 21);

/// SHA-256 of `(w, h, x, y)` and every u16 sample of layer 0 after each
/// edit, little-endian, captured at HEAD before the uniform branch existed.
/// A `rotate_canvas` case hashes its offset in the canvas of that time, the
/// symmetric box (a later change moved the old canvas's top-left by an
/// integer, [`placement_shift`], and left every sample where it was).
const PINS: &[(&str, &str)] = &[
    (
        "v0 rotate_canvas 17 nearest",
        "c42db9eb589df2056371fe50532082be22522f903b380ab29b3b710b43e74d19",
    ),
    (
        "v0 rotate_canvas 17 bilinear",
        "07eedcf782a18c628755efee6e0a4f532b4b9916bef67c3f5bd78806fbcf319e",
    ),
    (
        "v0 rotate_canvas 17 bicubic",
        "43f780295a453acb65f05ca8877b455ce0a058f323ea7fd6319a154b2bdfbec3",
    ),
    (
        "v0 rotate_canvas 17 lanczos3",
        "1ee07eeedaf77759623264888a36ae3f95a799bd672b46f34fff5c6dd64bf4da",
    ),
    (
        "v0 rotate_canvas -33.3 nearest",
        "83a2c0ca04265cc891695ffd9378b37002c295e6ce633a2980139c1b74e062e7",
    ),
    (
        "v0 rotate_canvas -33.3 bilinear",
        "3919c51f8f26b8874fca5326dd4fb798e08f7baf3167fe6406626ffc6448f82e",
    ),
    (
        "v0 rotate_canvas -33.3 bicubic",
        "79e78b082c805fc41e698cd1a2fad7f32d325a3b5ff6de35c3aa311ecfdd903e",
    ),
    (
        "v0 rotate_canvas -33.3 lanczos3",
        "6c593151543beda4d07176fbe5c6fd5b6cf5f5a35e1461bdc97799ee65363a91",
    ),
    (
        "v0 straighten 7",
        "9e92c0b01f6a87855072b21b24ec3dd515b0939e38b618db362856844c9c9eaf",
    ),
    (
        "v0 affine 17",
        "42422dcd2fd3f687368d35bba92c5b01187cb3534cf64e0294321b31182432d7",
    ),
    (
        "v0 perspective",
        "a2f6800c0d4e3d7de62730495beb6499a03491f8aacee40dadda47fae374aca9",
    ),
    (
        "v0 rotate90",
        "63cc1d9410f30306da87c7638b7909b6c02615f62d90aafe48070a0ee84f9120",
    ),
    (
        "v0 flip_horizontal",
        "e80f790604c908e9864739f0ac3e6e787d213440f900338ecbaeaf432ea0373e",
    ),
    (
        "v1 rotate_canvas 17 nearest",
        "2984030e8f96a7a06016e28884e549824d15e0d2139c7d15d2f7cd64811b963c",
    ),
    (
        "v1 rotate_canvas 17 bilinear",
        "facf617b671d2291b09497ec7da967d3bcdc6e531454a41674c71ea84b7a7db9",
    ),
    (
        "v1 rotate_canvas 17 bicubic",
        "c5c8cb116b269209b1f9b39d0bb7c593dcef2c31bc6199a7cb929a8e2714a256",
    ),
    (
        "v1 rotate_canvas 17 lanczos3",
        "a2ac49644756cb5b2245e508c28eb0b8beaf2cb5274a73d8ee660b57c6941684",
    ),
    (
        "v1 rotate_canvas -33.3 nearest",
        "f98943faac3beb7f7592ed9c8cc14ff207235aeeaf647ea8b8fe4d075c036ffc",
    ),
    (
        "v1 rotate_canvas -33.3 bilinear",
        "2bfc8540487f73853a5efd0568c609ca4bdd8777195a9bbdc0ffb60dd50be769",
    ),
    (
        "v1 rotate_canvas -33.3 bicubic",
        "8455cf1dcef6f883dd9c49e5da15ac14338020fb26b3a5ece1fc9eeee5513363",
    ),
    (
        "v1 rotate_canvas -33.3 lanczos3",
        "c86c513c01518a98e9414f9469503bf70ad29ac8616f561c1028e7d182a693ef",
    ),
    (
        "v1 straighten 7",
        "a8191684b274b5f6a97b69dc63c33359db5d4c339eb3f2d3e981ba2cc917084c",
    ),
    (
        "v1 affine 17",
        "b91d82308289037c297a3a171a9c34ace2c24cbe7063f482862d08fe5ff128e6",
    ),
    (
        "v1 perspective",
        "d8de0aab6f1cc666898814592a8c34d026d36497d6296f86b52281d1e3c3c6f7",
    ),
    (
        "v1 rotate90",
        "37fed9cc24598ce635a73d91fd614a0efdd88f4803a2e54fda231252a9bae9ad",
    ),
    (
        "v1 flip_horizontal",
        "24d8e57b608f12b995e5b0d2a653142993783f4008d7bfb53d5dfd2786bbceb1",
    ),
    (
        "v2 rotate_canvas 17 nearest",
        "82abd95c7d9af942964b5166487dfc2bde554ac88863c57d4154b5df91df0779",
    ),
    (
        "v2 rotate_canvas 17 bilinear",
        "f5439d90c77a4be5c3e72dd05c24aa271bf8a5bc223962f22b27c86d0f726b97",
    ),
    (
        "v2 rotate_canvas 17 bicubic",
        "f5439d90c77a4be5c3e72dd05c24aa271bf8a5bc223962f22b27c86d0f726b97",
    ),
    (
        "v2 rotate_canvas 17 lanczos3",
        "f5439d90c77a4be5c3e72dd05c24aa271bf8a5bc223962f22b27c86d0f726b97",
    ),
    (
        "v2 rotate_canvas -33.3 nearest",
        "dd93c5fc5d30a9346ec09a005aa54bac2f84bca22e193c174508d33e9d1fe88b",
    ),
    (
        "v2 rotate_canvas -33.3 bilinear",
        "d76f2aca0678194ab286d225cd8883ee2ca4fa54cd325f04c8e03e5ed38cfa32",
    ),
    (
        "v2 rotate_canvas -33.3 bicubic",
        "d76f2aca0678194ab286d225cd8883ee2ca4fa54cd325f04c8e03e5ed38cfa32",
    ),
    (
        "v2 rotate_canvas -33.3 lanczos3",
        "d76f2aca0678194ab286d225cd8883ee2ca4fa54cd325f04c8e03e5ed38cfa32",
    ),
    (
        "v2 straighten 7",
        "fb4a93aeac6d6d299bd5057bea2a1efbf94ab2ff8b62957537e2bc479a69608a",
    ),
    (
        "v2 affine 17",
        "0a57c8de8638e78b066bfd2af47e39111779eca7d4c7d129059cbca193dc5413",
    ),
    (
        "v2 perspective",
        "5aac28cc61565f524942e876362037f290f638eda82ee4afdcdfa09703f8e71e",
    ),
    (
        "v2 rotate90",
        "e4a50b40e96e18debbe58e6795b61e56d1f445be4cdca723ce80f91d64dcc743",
    ),
    (
        "v2 flip_horizontal",
        "eed3b518635a4130b8427c978eac9bd0d9a682b81edc928e7fe4f74dfa743e8e",
    ),
];

fn parse(request: &Value) -> GeometryEdit {
    GeometryEdit::parse(&request.to_string()).unwrap()
}

fn rotation(degrees: f64, cx: f64, cy: f64) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c]
}

/// The integer `old_origin − new_origin` a filtered Rotate Canvas of
/// `CANVAS` moved the old canvas's top-left by, both written
/// out here: the old origin is minus the floor of the four corners' smallest
/// coordinate under the rotation about the centre (the symmetric box), the
/// new one `⌊(⌈extent⌉ − W)/2⌋` on each axis. Corners are snapped within 1e-9.
fn placement_shift(degrees: f64) -> (i32, i32) {
    let (w, h) = (f64::from(CANVAS.0), f64::from(CANVAS.1));
    let m = rotation(degrees, w / 2.0, h / 2.0);
    let snap = |v: f64| {
        if (v - v.round()).abs() <= 1e-9 {
            v.round()
        } else {
            v
        }
    };
    let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|(x, y)| {
        (
            snap(m[0] * x + m[2] * y + m[4]),
            snap(m[1] * x + m[3] * y + m[5]),
        )
    });
    let axis = |vs: [f64; 4], old: f64| {
        let min = vs.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = vs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let old_origin = -(min.floor() as i64);
        let new_origin = (snap(max - min).ceil() as i64 - old as i64).div_euclid(2);
        (old_origin - new_origin) as i32
    };
    (axis(corners.map(|c| c.0), w), axis(corners.map(|c| c.1), h))
}

/// Every edit (a), named.
fn edits() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for degrees in [17.0, -33.3] {
        for filter in ["nearest", "bilinear", "bicubic", "lanczos3"] {
            out.push((
                format!("rotate_canvas {degrees} {filter}"),
                json!({"op": "rotate_canvas", "degrees": degrees, "filter": filter}),
            ));
        }
    }
    let window = (40_u32, 30_u32, 720_u32, 540_u32);
    let (cx, cy) = (
        f64::from(window.0) + f64::from(window.2) / 2.0,
        f64::from(window.1) + f64::from(window.3) / 2.0,
    );
    out.push((
        "straighten 7".into(),
        json!({"op": "straighten", "matrix": rotation(-7.0, cx, cy), "filter": "bicubic",
               "x": window.0, "y": window.1, "width": window.2, "height": window.3}),
    ));
    out.push((
        "affine 17".into(),
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": rotation(17.0, 400.5, 300.25), "filter": "bicubic"}),
    ));
    let (x0, y0) = (f64::from(OFFSET.0), f64::from(OFFSET.1));
    let (x1, y1) = (x0 + f64::from(RASTER.0), y0 + f64::from(RASTER.1));
    out.push((
        "perspective".into(),
        json!({"op": "perspective", "layer": 0, "target": "pixels",
               "quad": [x0 + 10.0, y0 + 5.0, x1 - 20.0, y0 + 30.0,
                        x1 + 40.0, y1 - 10.0, x0 - 15.0, y1 + 20.0],
               "filter": "bicubic"}),
    ));
    out.push(("rotate90".into(), json!({"op": "rotate90"})));
    out.push(("flip_horizontal".into(), json!({"op": "flip_horizontal"})));
    out
}

fn doc_of(image: PrecisionImage, canvas: (u32, u32), offset: (i32, i32)) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution { x: 72.0, y: 72.0 },
        vec![Layer::raster("Background", image, offset, 1.0, true).unwrap()],
    )
    .unwrap()
}

fn image_of(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn far(value: [u16; 4]) -> [u16; 4] {
    value.map(|c| 65535 - c)
}

/// The uniform raster (`corner` None) or its twin with one corner set far.
fn fixture(value: [u16; 4], corner: Option<usize>) -> PrecisionDocument {
    let (w, h) = RASTER;
    let mut pixels = vec![value; (w * h) as usize];
    if let Some(i) = corner {
        pixels[i] = far(value);
    }
    doc_of(image_of(w, h, pixels), CANVAS, OFFSET)
}

fn turned(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
    doc.geometry(&parse(request), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

struct Out {
    dims: (u32, u32),
    offset: (i32, i32),
    samples: Vec<[u16; 4]>,
}

fn out(doc: &PrecisionDocument) -> Out {
    let image = doc.layer_source(0).unwrap();
    Out {
        dims: image.dimensions(),
        offset: doc.layer(0).unwrap().offset(),
        samples: image.to_rgba16().unwrap(),
    }
}

fn digest(out: &Out) -> String {
    let mut bytes = Vec::with_capacity(16 + out.samples.len() * 8);
    bytes.extend_from_slice(&out.dims.0.to_le_bytes());
    bytes.extend_from_slice(&out.dims.1.to_le_bytes());
    bytes.extend_from_slice(&out.offset.0.to_le_bytes());
    bytes.extend_from_slice(&out.offset.1.to_le_bytes());
    for sample in &out.samples {
        for c in sample {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
    }
    sha256::sha256_hex(&bytes)
}

#[test]
fn a_uniform_raster_resamples_bit_identically() {
    let (w, h) = RASTER;
    let mut actual = Vec::new();
    for (v, value) in VALUES.into_iter().enumerate() {
        let uniform = fixture(value, None);
        let first = fixture(value, Some(0));
        let last = fixture(value, Some((w * h - 1) as usize));
        for (name, request) in edits() {
            let case = format!("v{v} {name}");
            let u = out(&turned(&uniform, &request));
            let n1 = out(&turned(&first, &request));
            let n2 = out(&turned(&last, &request));
            assert_eq!((u.dims, u.offset), (n1.dims, n1.offset), "{case}");
            assert_eq!((u.dims, u.offset), (n2.dims, n2.offset), "{case}");
            let mut differs = 0;
            for (i, ((a, b), c)) in u
                .samples
                .iter()
                .zip(&n1.samples)
                .zip(&n2.samples)
                .enumerate()
            {
                assert!(a == b || a == c, "{case}: pixel {i}");
                differs += usize::from(a != b);
            }
            assert!(differs <= 256, "{case}: {differs} pixels differ from N1");
            let mut pinned = u;
            if let Some(degrees) = request["degrees"].as_f64() {
                let shift = placement_shift(degrees);
                let want = if degrees == 17.0 { (1, 0) } else { (1, 1) };
                assert_eq!(shift, want, "{case}: the placement shift");
                pinned.offset = (pinned.offset.0 + shift.0, pinned.offset.1 + shift.1);
            }
            actual.push((case, digest(&pinned)));
        }
    }
    let listing: String = actual
        .iter()
        .map(|(case, digest)| format!("    (\"{case}\", \"{digest}\"),\n"))
        .collect();
    assert_eq!(PINS.len(), actual.len(), "pins:\n{listing}");
    for ((case, digest), (pin_case, pin)) in actual.iter().zip(PINS) {
        assert_eq!(case, pin_case);
        assert_eq!(digest, pin, "{case}");
    }
}

fn r17() -> GeometryEdit {
    parse(&json!({"op": "rotate_canvas", "degrees": 17, "filter": "bicubic"}))
}

fn float_doc(w: u32, h: u32, pixels: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width: w,
        height: h,
        channels: 4,
        alpha: Some(false),
        samples: pixels.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("uniform.tiff");
    std::fs::write(&path, encode_to_vec(&tiff, false).unwrap()).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(doc.is_float());
    doc
}

/// The non-uniform N1 raster carrying a `w`×`h` mask of `samples`.
fn masked(samples: Vec<u16>) -> PrecisionDocument {
    let (w, h) = RASTER;
    let mut pixels = vec![VALUES[1]; (w * h) as usize];
    pixels[0] = far(VALUES[1]);
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, samples, None).unwrap(),
        MaskProperties::at(OFFSET),
        true,
    )
    .unwrap();
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution { x: 72.0, y: 72.0 },
        vec![
            Layer::raster("Masked", image_of(w, h, pixels), OFFSET, 1.0, true)
                .unwrap()
                .with_mask(Some(mask)),
        ],
    )
    .unwrap()
}

/// R17 bicubic plan figures at HEAD (before the uniform branch), for the
/// paths this change must not move.
const HEAD_U16_R17: (u64, u64) = (31_438_500, 71_609_616);
const HEAD_FLOAT_R17: (u64, u64) = (57_672_700, 71_686_368);
const HEAD_MASK_R17: (u64, u64) = (33_924_945, 89_512_020);
/// What the plan's per-record bookkeeping term (`size_of::<LayerWork>()` per
/// record) grew by for these one-record documents: the new
/// `PlaneWork::uniform` flag adds 8 B to each of a record's two plane slots,
/// and a filtered turn takes 5/4 of it. The only scratch these plans move.
const RECORD_GROWTH: u64 = 20;
/// What the same term shrank by when the retained-source record lost its
/// live-filter cache, live-filter stack and Stack Mode fields (2026-10-10):
/// 32 B per record, of which a filtered turn takes 5/4.
const RECORD_SHRINK: u64 = 40;
/// What the same term grew by when `LayerWork` gained `anchor_shift`
/// (Delete Cropped Pixels' pattern-anchor rebase, two i64, 16 B per record),
/// of which a filtered turn takes 5/4.
const ANCHOR_SHIFT_GROWTH: u64 = 20;
/// What the same term grew by with the layer reference point:
/// `LayerWork::reference_shift` (two i64, 16 B) and the record's own
/// `reference` (two i32, 8 B, counted twice by the term), 32 B per record, of
/// which a filtered turn takes 5/4.
const REFERENCE_SHIFT_GROWTH: u64 = 40;
/// What the same term grew by when `reference_shift` became
/// `LayerWork::reference_map` (`Option<[f64; 6]>`, 56 B, in
/// place of the 16 B `(i64, i64)`): 40 B per record, of which a filtered
/// turn takes 5/4.
const REFERENCE_MAP_GROWTH: u64 = 50;
/// What the same term grew by when the record gained its Generative Layer
/// binding (`generated`): 192 B per record, of which a
/// filtered turn takes 5/4.
const RECORD_GENERATED: u64 = 240;
/// What the same term grew by when the record's Blend If gained its extra
/// ranges (`BlendIf::extra`, seven optional ranges, 63 B and
/// 64 B with the record's padding, counted twice by the term): 128 B per
/// record, of which a filtered turn takes 5/4.
const RECORD_BLEND_IF: u64 = 160;

fn at_head(head: (u64, u64)) -> (u64, u64) {
    (
        head.0
            + RECORD_GROWTH
            + ANCHOR_SHIFT_GROWTH
            + REFERENCE_SHIFT_GROWTH
            + REFERENCE_MAP_GROWTH
            + RECORD_GENERATED
            + RECORD_BLEND_IF
            - RECORD_SHRINK,
        head.1,
    )
}

fn plan_of(doc: &PrecisionDocument, edit: &GeometryEdit) -> (u64, u64) {
    let plan = doc.geometry_plan(edit).unwrap();
    (plan.scratch_bytes, plan.work_units)
}

#[test]
fn a_uniform_turn_shares_its_uniform_tiles() {
    for value in VALUES {
        for (name, request) in edits() {
            if !(name.contains("bicubic") || name == "affine 17" || name == "rotate90") {
                continue;
            }
            let result = turned(&fixture(value, None), &request);
            let image = result.layer_source(0).unwrap();
            let (w, h) = image.dimensions();
            let samples = image.to_rgba16().unwrap();
            // The profile's own records: a 1×1 image over the same profile
            // retains one 8-byte tile, one 8-byte index slot and the profile.
            let profile =
                PrecisionImage::from_rgba16(1, 1, vec![[0; 4]], image.profile().clone(), None)
                    .unwrap()
                    .retained_bytes()
                    - 16;
            let mut uniform = HashSet::new();
            let mut duplicates = 0;
            let mut bytes = profile;
            for ty in 0..h.div_ceil(TILE) {
                for tx in 0..w.div_ceil(TILE) {
                    let (x0, y0) = (tx * TILE, ty * TILE);
                    let (tw, th) = ((w - x0).min(TILE), (h - y0).min(TILE));
                    let first = samples[(y0 * w + x0) as usize];
                    let same = (y0..y0 + th)
                        .all(|y| (x0..x0 + tw).all(|x| samples[(y * w + x) as usize] == first));
                    let tile = u64::from(tw) * u64::from(th) * 8;
                    if !same || uniform.insert((tw, th, first)) {
                        bytes += tile;
                    } else {
                        duplicates += 1;
                    }
                    bytes += 8;
                }
            }
            assert_eq!(image.retained_bytes(), bytes, "{value:?} {name}");
            // HEAD kept every tile fresh, so a shared duplicate is what makes
            // this fail there.
            assert!(duplicates > 0, "{value:?} {name}");
        }
    }
}

#[test]
fn a_uniform_plane_plans_its_band() {
    let r17 = r17();
    let uniform = plan_of(&fixture(VALUES[0], None), &r17);
    let twin = plan_of(&fixture(VALUES[0], Some(0)), &r17);
    assert_eq!(twin, at_head(HEAD_U16_R17));
    assert_eq!(uniform.1, twin.1, "the work units are unchanged");
    assert!(uniform.0 < twin.0, "{uniform:?} {twin:?}");

    let (w, h) = RASTER;
    let n = (w * h) as usize;
    let white = vec![[1.0_f32; 4]; n];
    let mut white_twin = white.clone();
    white_twin[0] = [0.0, 0.0, 0.0, 1.0];
    assert_eq!(
        plan_of(&float_doc(w, h, &white), &r17),
        at_head(HEAD_FLOAT_R17)
    );
    assert_eq!(
        plan_of(&float_doc(w, h, &white_twin), &r17),
        at_head(HEAD_FLOAT_R17)
    );

    assert_eq!(
        plan_of(&masked(vec![40000; n]), &r17),
        at_head(HEAD_MASK_R17)
    );
    let mut mask_twin = vec![40000; n];
    mask_twin[0] = 1;
    assert_eq!(plan_of(&masked(mask_twin), &r17), at_head(HEAD_MASK_R17));
}

/// File ▸ New's white `w`×`h` 16-bit background (`create_blank`) with a
/// non-uniform `w`×`h` photo placed above it (`native_banded_reopen.rs`'s
/// `place` request).
fn two_layer(w: u32, h: u32) -> PrecisionDocument {
    let srgb = IccProfile::srgb();
    let background =
        PrecisionDocument::create_blank(w, h, [65535; 4], srgb.bytes(), "Background", LIMIT, 0)
            .unwrap();
    let photo = PrecisionDocument::from_image(
        image_of(
            w,
            h,
            (0..w * h)
                .map(|i| {
                    let (x, y) = ((i % w) as u16, (i / w) as u16);
                    [x.wrapping_mul(10), y.wrapping_mul(16), x ^ y, 65535]
                })
                .collect(),
        ),
        Metadata::default(),
        Resolution { x: 72.0, y: 72.0 },
    )
    .unwrap();
    background
        .source_edit(
            Some(&photo),
            &SourceEdit::parse(
                r#"{"op":"place","source_id":"uniform-turn","name":"Photo","x":0,"y":0}"#,
            )
            .unwrap(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap()
}

/// ⌈1.25 × the measured max E⌉ of (U), the white 6000×4000 background
/// alone, and of (2L) (`precision_geometry_budget`'s two-layer record).
const UNIFORM_FLOOR: u64 = 26_665_020;
const TWO_LAYER_FLOOR: u64 = 1_026_376_460;

#[test]
fn a_two_layer_24_mp_turn_plan_covers_its_peak() {
    let (w, h) = (6000_u32, 4000_u32);
    let background = PrecisionDocument::create_blank(
        w,
        h,
        [65535; 4],
        IccProfile::srgb().bytes(),
        "U",
        LIMIT,
        0,
    )
    .unwrap();
    let plan = background.geometry_plan(&r17()).unwrap();
    assert!(
        plan.scratch_bytes >= UNIFORM_FLOOR,
        "{}",
        plan.scratch_bytes
    );
    let doc = two_layer(w, h);
    let plan = doc.geometry_plan(&r17()).unwrap();
    let total = plan.retained_bytes + plan.scratch_bytes;
    println!("R17: retained + scratch {total}");
    assert!(total <= MINIMUM_MAC_LIMIT);
    assert!(
        plan.scratch_bytes >= TWO_LAYER_FLOOR,
        "{}",
        plan.scratch_bytes
    );

    let matrix = rotation(17.0, 3000.0, 2000.0);
    for request in [
        json!({"op":"affine_set","layers":[1],"matrix":matrix,"filter":"bicubic"}),
        json!({"op":"perspective","layer":1,"target":"pixels",
               "quad":[0, 0, 6000, 0, 6900, 4600, -900, 4600],"filter":"bicubic"}),
    ] {
        let plan = doc.geometry_plan(&parse(&request)).unwrap();
        let total = plan.retained_bytes + plan.scratch_bytes;
        println!("{}: retained + scratch {total}", request["op"]);
        assert!(total <= MINIMUM_MAC_LIMIT, "{request}");
    }
}

#[test]
fn a_turned_two_layer_document_reopens_bit_exact() {
    let turned = turned(
        &two_layer(640, 480),
        &json!({"op":"rotate_canvas","degrees":17,"filter":"bicubic"}),
    );
    let plan = turned.native_plan().unwrap();
    let bytes = turned.encode_native(LIMIT, 0, None).unwrap();
    let back = PrecisionDocument::parse_native(
        &bytes,
        plan.reopen_retained_bytes + plan.decode_scratch_bytes,
        0,
        None,
    )
    .unwrap();
    for layer in 0..2 {
        let (a, b) = (out_layer(&turned, layer), out_layer(&back, layer));
        assert_eq!((a.dims, a.offset), (b.dims, b.offset), "layer {layer}");
        assert!(a.samples == b.samples, "layer {layer}");
    }
}

fn out_layer(doc: &PrecisionDocument, index: usize) -> Out {
    let image = doc.layer_source(index).unwrap();
    Out {
        dims: image.dimensions(),
        offset: doc.layer(index).unwrap().offset(),
        samples: image.to_rgba16().unwrap(),
    }
}

#[test]
fn a_uniform_turn_cancels_and_reports_to_the_end() {
    let doc = fixture(VALUES[0], None);
    let before = doc.layer_source(0).unwrap().clone();
    let control = DrJobControl::default();
    control.cancel();
    let cancelled = match doc.geometry(&r17(), LIMIT, 0, Some(&control)) {
        Err(message) => message,
        Ok(_) => panic!("a cancelled turn must refuse"),
    };
    assert!(cancelled.contains("cancel"), "{cancelled}");
    assert!(PrecisionImage::ptr_eq(
        doc.layer_source(0).unwrap(),
        &before
    ));
    assert!(doc.layer_source(0).unwrap().same_samples(&before));

    let control = DrJobControl::default();
    control.record_reports();
    doc.geometry(&r17(), LIMIT, 0, Some(&control))
        .unwrap()
        .unwrap();
    let last = control.reports().last().copied().expect("a report");
    assert_eq!(last.progress, 1.0);
}
