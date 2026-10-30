//! The Remove tool's model engine (M3a) through the FFI:
//! `dr_doc_remove_model_*` and `dr_precision_doc_remove_model_*` at 8 and 16
//! bits, with a STUB plane in place of the model.
//!
//! # The oracles
//!
//! Every figure is written here from the header's contract: the crop rule;
//! the native mask (any covered pixel in the cell's integer footprint) and
//! image (the area-weighted, alpha-weighted mean, by brute force over each
//! cell's fractional footprint); the placement through a plane that encodes
//! its own coordinates, whose bilinear read is the ramp itself; the
//! coverage composite `A = c + (1 − c)·a0`, `RGB = (c·f + (1 − c)·a0·rgb0)/A`.
//!
//! # Bounds, declared before the first run
//!
//! * Native cells: 1e-5 (f32 output of an f64 sum).
//! * Placement and composite: ±1 code at 8 and 16 bits (one rounding).
//! * Uncovered pixels: bit-exact.
#![allow(clippy::needless_range_loop)]

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::LayerKind;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_remove_model::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::layer_raster::LayerRaster;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::retouch_sampling::RetouchSampling;
use image::{Rgba, RgbaImage};
use serde_json::json;

use crate::cmyk_fixture::press_profile;
use crate::common::{layer_bytes, take_err_string};
use crate::float_fixture::float_doc;
use crate::sky_replacement::{build, layer_count, typed_layer, Depth, Doc, LIMIT};

const SIDE: u32 = 64;
const DEPTHS: [Depth; 2] = [Depth::Byte, Depth::Sixteen];

// ---------------------------------------------------------------- calls --

fn sample_name(sample: &str) -> CString {
    CString::new(sample).unwrap()
}

fn plan(doc: &Doc, layer: usize, sample: &str, new_layer: bool) -> Result<u64, String> {
    let name = sample_name(sample);
    let mut err = ptr::null_mut();
    let bytes = unsafe {
        match doc {
            Doc::Byte(d) => {
                dr_doc_remove_model_plan(d, layer, name.as_ptr(), SIDE, new_layer, &mut err)
            }
            Doc::Typed(d) => dr_precision_doc_remove_model_plan(
                d,
                layer,
                name.as_ptr(),
                SIDE,
                new_layer,
                &mut err,
            ),
        }
    };
    if bytes == 0 {
        Err(take_err_string(err))
    } else {
        assert!(err.is_null());
        Ok(bytes)
    }
}

struct Input {
    crop: [u32; 4],
    image: Vec<f32>,
    mask: Vec<f32>,
}

#[allow(clippy::too_many_arguments)]
fn input_with(
    doc: &Doc,
    layer: usize,
    sample: &str,
    ignore: bool,
    coverage: &[u16],
    side: u32,
    limit: u64,
    control: *const DrJobControl,
) -> Result<Input, String> {
    let name = sample_name(sample);
    let cells = (side * side) as usize;
    let mut crop = [0u32; 4];
    let mut image = vec![0f32; 3 * cells];
    let mut mask = vec![0f32; cells];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_remove_model_input(
                d,
                layer,
                name.as_ptr(),
                ignore,
                coverage.as_ptr(),
                coverage.len(),
                side,
                crop.as_mut_ptr(),
                image.as_mut_ptr(),
                image.len(),
                mask.as_mut_ptr(),
                mask.len(),
                limit,
                0,
                control,
                &mut err,
            ),
            Doc::Typed(d) => dr_precision_doc_remove_model_input(
                d,
                layer,
                name.as_ptr(),
                ignore,
                coverage.as_ptr(),
                coverage.len(),
                side,
                crop.as_mut_ptr(),
                image.as_mut_ptr(),
                image.len(),
                mask.as_mut_ptr(),
                mask.len(),
                limit,
                0,
                control,
                &mut err,
            ),
        }
    };
    if ok {
        assert!(err.is_null());
        Ok(Input { crop, image, mask })
    } else {
        Err(take_err_string(err))
    }
}

fn input(doc: &Doc, layer: usize, sample: &str, coverage: &[u16]) -> Result<Input, String> {
    input_with(
        doc,
        layer,
        sample,
        false,
        coverage,
        SIDE,
        LIMIT,
        ptr::null(),
    )
}

fn apply_with(
    doc: &Doc,
    layer: usize,
    coverage: &[u16],
    plane: &[f32],
    new_layer: bool,
    limit: u64,
) -> Result<Option<Doc>, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        match doc {
            Doc::Byte(d) => {
                let out = dr_doc_remove_model_apply(
                    d,
                    layer,
                    coverage.as_ptr(),
                    coverage.len(),
                    SIDE,
                    plane.as_ptr(),
                    plane.len(),
                    new_layer,
                    limit,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Byte(*Box::from_raw(out)))
            }
            Doc::Typed(d) => {
                let out = dr_precision_doc_remove_model_apply(
                    d,
                    layer,
                    coverage.as_ptr(),
                    coverage.len(),
                    SIDE,
                    plane.as_ptr(),
                    plane.len(),
                    new_layer,
                    limit,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Typed(*Box::from_raw(out)))
            }
        }
    };
    match out {
        Some(doc) => {
            assert!(err.is_null());
            Ok(Some(doc))
        }
        None if err.is_null() => Ok(None),
        None => Err(take_err_string(err)),
    }
}

fn apply(
    doc: &Doc,
    layer: usize,
    coverage: &[u16],
    plane: &[f32],
    new_layer: bool,
) -> Result<Option<Doc>, String> {
    apply_with(doc, layer, coverage, plane, new_layer, LIMIT)
}

// -------------------------------------------------------------- helpers --

/// Layer `idx`'s own samples as unit floats, with its size.
fn unit_layer(doc: &Doc, idx: usize) -> (Vec<[f64; 4]>, (u32, u32)) {
    match doc {
        Doc::Byte(d) => (
            layer_bytes(d, idx)
                .chunks(4)
                .map(|p| [0, 1, 2, 3].map(|c| f64::from(p[c]) / 255.0))
                .collect(),
            d.layers[idx].pixels.dimensions(),
        ),
        Doc::Typed(d) => {
            let image = d.layer(idx).unwrap().source().unwrap();
            (
                image
                    .to_rgba16()
                    .unwrap()
                    .iter()
                    .map(|p| p.map(|v| f64::from(v) / 65535.0))
                    .collect(),
                image.dimensions(),
            )
        }
    }
}

fn layer_name(doc: &Doc, idx: usize) -> String {
    match doc {
        Doc::Byte(d) => d.layers[idx].name.clone(),
        Doc::Typed(d) => d.layer(idx).unwrap().name().to_string(),
    }
}

/// The value a depth stores for a unit value, back in unit terms.
fn stored(depth: Depth, v: f64) -> f64 {
    (v.clamp(0.0, 1.0) * depth.max()).round() / depth.max()
}

fn uniform(w: u32, h: u32, pixel: [f64; 4]) -> Vec<[f64; 4]> {
    vec![pixel; (w * h) as usize]
}

/// Coverage `value` inside the disc of radius `r` at `(cx, cy)`.
fn disc(w: u32, h: u32, (cx, cy): (f64, f64), r: f64, value: u16) -> Vec<u16> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (f64::from(i % w) + 0.5, f64::from(i / w) + 0.5);
            if (x - cx).powi(2) + (y - cy).powi(2) <= r * r {
                value
            } else {
                0
            }
        })
        .collect()
}

fn rect_coverage(w: u32, h: u32, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> Vec<u16> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (x0..x1).contains(&x) && (y0..y1).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect()
}

fn uniform_plane(f: [f32; 3]) -> Vec<f32> {
    let cells = (SIDE * SIDE) as usize;
    (0..3 * cells).map(|i| f[i / cells]).collect()
}

fn assert_codes(depth: Depth, got: f64, want: f64, what: &str) {
    let diff = (got - want).abs() * depth.max();
    assert!(
        diff <= 1.0 + 1e-6,
        "{what} at {depth:?}: got {got}, want {want} ({diff} codes)"
    );
}

/// The crop rule, restated from the header: `s = max(side, ⌈2L⌉)`, clipped
/// to the canvas, centred on the covered rect and slid inside the canvas.
fn crop_oracle(covered: (u32, u32, u32, u32), canvas: (u32, u32), side: u32) -> [u32; 4] {
    let (x0, y0, w, h) = covered;
    let s = side.max(2 * w.max(h));
    let (cw, ch) = (s.min(canvas.0), s.min(canvas.1));
    let x =
        (i64::from(x0) + i64::from(w / 2) - i64::from(cw / 2)).clamp(0, i64::from(canvas.0 - cw));
    let y =
        (i64::from(y0) + i64::from(h / 2) - i64::from(ch / 2)).clamp(0, i64::from(canvas.1 - ch));
    [x as u32, y as u32, cw, ch]
}

// ------------------------------------------------------------ geometry --

#[test]
fn the_crop_follows_its_rule_at_corners_edges_and_small_canvases() {
    let (w, h) = (200u32, 150u32);
    let cases = [
        (0, 0, 4, 4),
        (190, 70, 10, 10),
        (100, 60, 10, 6),
        (40, 70, 120, 5),
        (0, 140, 30, 10),
    ];
    for depth in DEPTHS {
        let doc = build(depth, w, h, &[uniform(w, h, [0.4, 0.5, 0.6, 1.0])]);
        for (x, y, cw, ch) in cases {
            let coverage = rect_coverage(w, h, (x, y, x + cw, y + ch));
            let got = input(&doc, 0, "current", &coverage).unwrap();
            assert_eq!(
                got.crop,
                crop_oracle((x, y, cw, ch), (w, h), SIDE),
                "{depth:?} {x},{y}"
            );
        }
        // A canvas smaller than the native side is the whole canvas.
        let small = build(depth, 40, 30, &[uniform(40, 30, [0.4, 0.5, 0.6, 1.0])]);
        let got = input(
            &small,
            0,
            "current",
            &rect_coverage(40, 30, (10, 10, 14, 13)),
        )
        .unwrap();
        assert_eq!(got.crop, [0, 0, 40, 30]);
    }
}

/// The native cells of `picture` (unit, `w` wide) and `coverage` over
/// `crop`, by brute force over each cell's fractional footprint.
fn cells_oracle(
    picture: &[[f64; 4]],
    coverage: &[u16],
    w: u32,
    crop: [u32; 4],
) -> (Vec<[f64; 3]>, Vec<f64>) {
    let [x, y, cw, ch] = crop.map(f64::from);
    let s = f64::from(SIDE);
    let mut image = vec![[0.0; 3]; (SIDE * SIDE) as usize];
    let mut mask = vec![0.0; (SIDE * SIDE) as usize];
    for j in 0..SIDE {
        for i in 0..SIDE {
            let (ax, bx) = (f64::from(i) * cw / s, f64::from(i + 1) * cw / s);
            let (ay, by) = (f64::from(j) * ch / s, f64::from(j + 1) * ch / s);
            let (mut sum, mut weight, mut any) = ([0.0; 3], 0.0, false);
            for py in ay.floor() as u32..by.ceil() as u32 {
                for px in ax.floor() as u32..bx.ceil() as u32 {
                    let wx = (bx.min(f64::from(px) + 1.0) - ax.max(f64::from(px))).max(0.0);
                    let wy = (by.min(f64::from(py) + 1.0) - ay.max(f64::from(py))).max(0.0);
                    let at = ((y as u32 + py) * w + x as u32 + px) as usize;
                    let p = picture[at];
                    let wa = wx * wy * p[3];
                    for c in 0..3 {
                        sum[c] += wa * p[c];
                    }
                    weight += wa;
                    any |= coverage[at] > 0;
                }
            }
            let k = (j * SIDE + i) as usize;
            image[k] = if weight > 0.0 {
                sum.map(|v| v / weight)
            } else {
                [0.0; 3]
            };
            mask[k] = if any { 1.0 } else { 0.0 };
        }
    }
    (image, mask)
}

#[test]
fn the_native_mask_and_image_are_their_hand_sums() {
    // A crop wider than the native side (80 px over 64 cells) and one
    // narrower (a 40 × 30 canvas over 64 cells): fractional footprints both.
    for depth in DEPTHS {
        for ((w, h), hole) in [
            ((120u32, 100u32), (40u32, 30u32, 80u32, 70u32)),
            ((40, 30), (10, 8, 18, 14)),
        ] {
            let picture: Vec<[f64; 4]> = (0..w * h)
                .map(|i| {
                    let (x, y) = (f64::from(i % w), f64::from(i / w));
                    let a = if (x as u32 + y as u32).is_multiple_of(7) {
                        0.0
                    } else {
                        0.3 + 0.7 * (x / f64::from(w))
                    };
                    [
                        x / f64::from(w),
                        y / f64::from(h),
                        ((x + y) % 13.0) / 13.0,
                        a,
                    ]
                })
                .map(|p| p.map(|v| stored(depth, v)))
                .collect();
            let doc = build(depth, w, h, std::slice::from_ref(&picture));
            let coverage = rect_coverage(w, h, hole);
            let got = input(&doc, 0, "current", &coverage).unwrap();
            let (image, mask) = cells_oracle(&picture, &coverage, w, got.crop);
            let cells = (SIDE * SIDE) as usize;
            for k in 0..cells {
                assert_eq!(got.mask[k], mask[k] as f32, "{depth:?} mask cell {k}");
                for c in 0..3 {
                    let diff = (f64::from(got.image[c * cells + k]) - image[k][c]).abs();
                    assert!(diff <= 1e-5, "{depth:?} cell {k} channel {c}: {diff}");
                }
            }
        }
    }
}

// ------------------------------------------------------------ composite --

#[test]
fn the_fill_lands_at_the_stroke_and_nothing_else_moves() {
    let (w, h) = (160u32, 120u32);
    for depth in DEPTHS {
        let picture: Vec<[f64; 4]> = (0..w * h)
            .map(|i| [f64::from(i % w) / 200.0, f64::from(i / w) / 150.0, 0.5, 1.0])
            .map(|p| p.map(|v| stored(depth, v)))
            .collect();
        let doc = build(depth, w, h, std::slice::from_ref(&picture));
        let coverage = disc(w, h, (80.0, 60.0), 10.0, 65535);
        let crop = input(&doc, 0, "current", &coverage).unwrap().crop;
        // A plane holding its own coordinates: R = (i + ½)/side, G = (j + ½)/side.
        let cells = (SIDE * SIDE) as usize;
        let plane: Vec<f32> = (0..3 * cells)
            .map(|n| {
                let (c, k) = (n / cells, n % cells);
                let (i, j) = ((k % SIDE as usize) as f32, (k / SIDE as usize) as f32);
                match c {
                    0 => (i + 0.5) / SIDE as f32,
                    1 => (j + 0.5) / SIDE as f32,
                    _ => 0.25,
                }
            })
            .collect();
        let out = apply(&doc, 0, &coverage, &plane, false).unwrap().unwrap();
        let (got, size) = unit_layer(&out, 0);
        assert_eq!(size, (w, h));
        let [x, y, cw, ch] = crop.map(f64::from);
        for i in 0..(w * h) as usize {
            let (px, py) = ((i as u32 % w) as f64, (i as u32 / w) as f64);
            if coverage[i] == 0 {
                assert_eq!(got[i], picture[i], "{depth:?} uncovered pixel {i} moved");
                continue;
            }
            assert_codes(depth, got[i][0], (px + 0.5 - x) / cw, "placement x");
            assert_codes(depth, got[i][1], (py + 0.5 - y) / ch, "placement y");
            assert_codes(depth, got[i][2], 0.25, "blue");
            assert_eq!(got[i][3], 1.0);
        }
    }
}

#[test]
fn a_soft_stroke_on_a_translucent_layer_is_the_closed_form() {
    let (w, h) = (90u32, 70u32);
    let fill = [0.1f32, 0.7, 0.3];
    for depth in DEPTHS {
        let under = [0.8, 0.4, 0.2, 0.5].map(|v| stored(depth, v));
        let doc = build(depth, w, h, &[uniform(w, h, under)]);
        let coverage = disc(w, h, (45.0, 35.0), 9.0, 26214);
        let out = apply(&doc, 0, &coverage, &uniform_plane(fill), false)
            .unwrap()
            .unwrap();
        let (got, _) = unit_layer(&out, 0);
        let c = 26214.0 / 65535.0;
        let a = c + (1.0 - c) * under[3];
        for i in 0..(w * h) as usize {
            if coverage[i] == 0 {
                assert_eq!(got[i], under);
                continue;
            }
            for k in 0..3 {
                let want = (c * f64::from(fill[k]) + (1.0 - c) * under[3] * under[k]) / a;
                assert_codes(depth, got[i][k], want, "colour");
            }
            assert_codes(depth, got[i][3], a, "alpha");
        }
    }
}

#[test]
fn new_layer_output_lands_directly_above_with_the_coverage_as_alpha() {
    let (w, h) = (90u32, 70u32);
    let fill = [0.1f32, 0.7, 0.3];
    for depth in DEPTHS {
        let base = [0.8, 0.4, 0.2, 1.0].map(|v| stored(depth, v));
        let top = [0.2, 0.2, 0.9, 1.0].map(|v| stored(depth, v));
        let doc = build(depth, w, h, &[uniform(w, h, base), uniform(w, h, top)]);
        let coverage = disc(w, h, (45.0, 35.0), 9.0, 40000);
        let out = apply(&doc, 0, &coverage, &uniform_plane(fill), true)
            .unwrap()
            .unwrap();
        assert_eq!(layer_count(&out), 3);
        assert_eq!(layer_name(&out, 1), "Remove");
        assert_eq!(layer_name(&out, 2), "L1");
        assert_eq!(unit_layer(&out, 0).0, unit_layer(&doc, 0).0);
        let (got, size) = unit_layer(&out, 1);
        assert_eq!(size, (w, h));
        let c = 40000.0 / 65535.0;
        for i in 0..(w * h) as usize {
            if coverage[i] == 0 {
                assert_eq!(got[i], [0.0; 4]);
                continue;
            }
            for k in 0..3 {
                assert_codes(depth, got[i][k], f64::from(fill[k]), "new layer colour");
            }
            assert_codes(depth, got[i][3], c, "new layer alpha");
        }
    }
}

#[test]
fn an_offset_layer_is_written_inside_its_extent_only() {
    let (w, h) = (100u32, 80u32);
    for depth in DEPTHS {
        let small: Vec<[f64; 4]> = uniform(40, 40, [0.3, 0.6, 0.9, 1.0].map(|v| stored(depth, v)));
        let doc = match build(depth, w, h, &[uniform(w, h, [0.5, 0.5, 0.5, 1.0])]) {
            Doc::Byte(mut d) => {
                d = d.adding_layer(0, "L1").unwrap();
                let image = RgbaImage::from_fn(40, 40, |x, y| {
                    Rgba(small[(y * 40 + x) as usize].map(|v| (v * 255.0).round() as u8))
                });
                d.layers[1].pixels = LayerRaster::new(image);
                d.layers[1].offset = (10, 10);
                Doc::Byte(d)
            }
            Doc::Typed(d) => {
                let base = d.layer(0).unwrap().clone();
                Doc::Typed(
                    PrecisionDocument::from_parts(
                        w,
                        h,
                        IccProfile::srgb(),
                        Metadata::default(),
                        Resolution::default(),
                        vec![base, typed_layer("L1", 40, 40, &small, (10, 10))],
                    )
                    .unwrap(),
                )
            }
        };
        // The stroke crosses the layer's right edge (x = 50).
        let coverage = disc(w, h, (50.0, 30.0), 8.0, 65535);
        let out = apply(&doc, 1, &coverage, &uniform_plane([0.0, 0.0, 0.0]), false)
            .unwrap()
            .unwrap();
        let (got, size) = unit_layer(&out, 1);
        assert_eq!(size, (40, 40), "{depth:?}: the layer never grows");
        for ly in 0..40u32 {
            for lx in 0..40u32 {
                let k = (ly * 40 + lx) as usize;
                let canvas = ((ly + 10) * w + lx + 10) as usize;
                if coverage[canvas] == 0 {
                    assert_eq!(got[k], small[k]);
                } else {
                    assert_eq!(got[k], [0.0, 0.0, 0.0, 1.0]);
                }
            }
        }
    }
}

#[test]
fn a_plane_that_reproduces_the_pixels_changes_nothing() {
    let (w, h) = (80u32, 60u32);
    for depth in DEPTHS {
        let doc = build(depth, w, h, &[uniform(w, h, [0.2, 0.2, 0.2, 1.0])]);
        let coverage = disc(w, h, (40.0, 30.0), 7.0, 65535);
        assert!(
            apply(&doc, 0, &coverage, &uniform_plane([0.2, 0.2, 0.2]), false)
                .unwrap()
                .is_none()
        );
    }
}

// -------------------------------------------------------------- refusals --

#[test]
fn each_refusal_names_itself() {
    let (w, h) = (80u32, 60u32);
    let coverage = disc(w, h, (40.0, 30.0), 7.0, 65535);
    let plane = uniform_plane([0.5, 0.5, 0.5]);
    for depth in DEPTHS {
        let doc = build(
            depth,
            w,
            h,
            &[
                uniform(w, h, [0.5, 0.4, 0.3, 1.0]),
                uniform(w, h, [0.1, 0.2, 0.3, 1.0]),
            ],
        );
        let range = "remove layer 5 is out of range";
        assert_eq!(plan(&doc, 5, "all", false).unwrap_err(), range);
        assert_eq!(input(&doc, 5, "all", &coverage).err().unwrap(), range);
        assert_eq!(
            apply(&doc, 5, &coverage, &plane, false).err().unwrap(),
            range
        );
        assert_eq!(
            input(&doc, 0, "everything", &coverage).err().unwrap(),
            "sample must be current, current_and_below or all"
        );
        let buffer = "invalid remove coverage buffer";
        assert_eq!(input(&doc, 0, "all", &coverage[1..]).err().unwrap(), buffer);
        assert_eq!(
            apply(&doc, 0, &coverage[1..], &plane, false).err().unwrap(),
            buffer
        );
        assert_eq!(
            apply(&doc, 0, &coverage, &plane[1..], false).err().unwrap(),
            "invalid remove model plane"
        );
        let empty = vec![0u16; coverage.len()];
        let nothing = "the stroke covers no pixel of the picture";
        assert_eq!(input(&doc, 0, "all", &empty).err().unwrap(), nothing);
        assert_eq!(
            apply(&doc, 0, &empty, &plane, false).err().unwrap(),
            nothing
        );
        let mut nan = plane.clone();
        nan[17] = f32::NAN;
        assert_eq!(
            apply(&doc, 0, &coverage, &nan, false).err().unwrap(),
            "remove model output is not finite"
        );
        assert_eq!(
            input_with(&doc, 0, "all", false, &coverage, 32, LIMIT, ptr::null())
                .err()
                .unwrap(),
            "remove model side must be 64…1024"
        );
        // A group and an adjustment layer are not raster layers.
        let grouped = match &doc {
            Doc::Byte(d) => {
                let (g, ..) = d.group_layers(&[0], "G").unwrap();
                let at = g
                    .layers
                    .iter()
                    .position(|l| l.kind == LayerKind::Group)
                    .unwrap();
                (Doc::Byte(g), at)
            }
            Doc::Typed(d) => {
                let (g, _) = d.group_layers(&[0], "G", false).unwrap().unwrap();
                let at = (0..g.layer_count())
                    .find(|&i| g.layer(i).unwrap().children().is_some())
                    .unwrap();
                (Doc::Typed(g), at)
            }
        };
        let not_raster = format!(
            "remove needs a raster layer; layer {} is not one",
            grouped.1
        );
        assert_eq!(
            plan(&grouped.0, grouped.1, "all", false).unwrap_err(),
            not_raster
        );
        assert_eq!(
            input(&grouped.0, grouped.1, "all", &coverage)
                .err()
                .unwrap(),
            not_raster
        );
        assert_eq!(
            apply(&grouped.0, grouped.1, &coverage, &plane, false)
                .err()
                .unwrap(),
            not_raster
        );
        let adjusted = with_adjustment(&doc);
        let top = layer_count(&adjusted) - 1;
        assert_eq!(
            apply(&adjusted, top, &coverage, &plane, false)
                .err()
                .unwrap(),
            format!("remove needs a raster layer; layer {top} is not one")
        );
        // A hole whose surround is transparent: a disc alone on its layer.
        let alone = build(
            depth,
            w,
            h,
            &[
                uniform(w, h, [0.5, 0.4, 0.3, 1.0]),
                (0..w * h)
                    .map(|i| {
                        if coverage[i as usize] > 0 {
                            [0.9, 0.1, 0.1, 1.0]
                        } else {
                            [0.0; 4]
                        }
                    })
                    .collect(),
            ],
        );
        assert_eq!(
            input(&alone, 1, "current", &coverage).err().unwrap(),
            "there is nothing to sample from: every pixel around the stroke is transparent."
        );
        // The same disc sampling the layers below has context.
        assert!(input(&alone, 1, "current_and_below", &coverage).is_ok());
        // A working limit below the plan, and a cancelled control.
        assert!(
            input_with(&doc, 0, "all", false, &coverage, SIDE, 4096, ptr::null())
                .err()
                .unwrap()
                .contains("working limit")
        );
        assert!(apply_with(&doc, 0, &coverage, &plane, false, 4096)
            .err()
            .unwrap()
            .contains("working limit"));
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let cancelled = input_with(&doc, 0, "all", false, &coverage, SIDE, LIMIT, control);
        unsafe { dr_job_control_free(control) };
        assert_eq!(cancelled.err().as_deref(), Some("operation cancelled"));
    }
    // 32-bit float and CMYK: refused by name at every export (no Photoshop
    // reference was recorded at either).
    let not_float = "remove's model is not available in a 32-bit float document";
    let not_cmyk = "remove's model is not available in a CMYK document";
    let n = (w * h) as usize;
    let float = Doc::Typed(float_doc(
        w,
        h,
        &IccProfile::srgb(),
        &[vec![[0.5, 0.5, 0.5, 1.0]; n]],
    ));
    assert_eq!(plan(&float, 0, "all", false).unwrap_err(), not_float);
    assert_eq!(input(&float, 0, "all", &coverage).err().unwrap(), not_float);
    assert_eq!(
        apply(&float, 0, &coverage, &plane, false).err().unwrap(),
        not_float
    );
    let rgb = match build(Depth::Sixteen, w, h, &[uniform(w, h, [0.5, 0.4, 0.3, 1.0])]) {
        Doc::Typed(d) => Box::into_raw(Box::new(d)),
        Doc::Byte(_) => unreachable!(),
    };
    let profile = press_profile("Press Remove");
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "{}", take_err_string(err));
    let cmyk_doc = Doc::Typed(unsafe { (*cmyk).clone() });
    assert_eq!(plan(&cmyk_doc, 0, "all", false).unwrap_err(), not_cmyk);
    assert_eq!(
        input(&cmyk_doc, 0, "all", &coverage).err().unwrap(),
        not_cmyk
    );
    assert_eq!(
        apply(&cmyk_doc, 0, &coverage, &plane, false).err().unwrap(),
        not_cmyk
    );
    unsafe {
        dr_precision_doc_free(cmyk);
        dr_precision_doc_free(rgb);
    }
}

#[test]
fn a_hole_over_the_whole_crop_has_no_context() {
    // Select All's coverage: every native cell is masked, so the refusal is
    // NO_CONTEXT, not the transparency sentence, whatever the surround.
    let no_context = "the hole covers the whole crop: there is no picture around it to fill from";
    for depth in DEPTHS {
        for (w, h) in [(40u32, 30u32), (120, 100)] {
            let doc = build(depth, w, h, &[uniform(w, h, [0.5, 0.4, 0.3, 1.0])]);
            let all = vec![65535u16; (w * h) as usize];
            for sample in ["current", "current_and_below", "all"] {
                assert_eq!(
                    input(&doc, 0, sample, &all).err().unwrap(),
                    no_context,
                    "{depth:?} {w}x{h} {sample}"
                );
            }
            let clear = build(depth, w, h, &[uniform(w, h, [0.0; 4])]);
            assert_eq!(input(&clear, 0, "current", &all).err().unwrap(), no_context);
            // Four uncovered columns leave a native cell unmasked: context.
            let mut most = all.clone();
            for y in 0..h {
                for x in 0..4 {
                    most[(y * w + x) as usize] = 0;
                }
            }
            assert!(
                input(&doc, 0, "current", &most).is_ok(),
                "{depth:?} {w}x{h}"
            );
        }
    }
}

#[test]
fn a_null_document_carries_the_standard_message_at_the_byte_apply() {
    let coverage = [65535u16; 4];
    let plane = uniform_plane([0.5, 0.5, 0.5]);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_remove_model_apply(
            ptr::null(),
            0,
            coverage.as_ptr(),
            coverage.len(),
            SIDE,
            plane.as_ptr(),
            plane.len(),
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), "required handle is NULL");
}

/// `doc` with an adjustment layer on top: Invert at 8 bits, an exposure of
/// +0.25 at 16.
fn with_adjustment(doc: &Doc) -> Doc {
    match doc {
        Doc::Byte(d) => {
            let mut a = d.adding_layer(d.layers.len() - 1, "Adj").unwrap();
            let top = a.layers.len() - 1;
            a.layers[top].meta = Some(r#"{"type":"adjust","op":"invert"}"#.into());
            Doc::Byte(a)
        }
        Doc::Typed(d) => {
            let description = json!({"type":"adjust","evaluation":"precision_v1",
                                     "op":"exposure","params":{"exposure":0.25}});
            Doc::Typed(
                d.add_adjustment_at(
                    "Adj",
                    &description.to_string(),
                    1.0,
                    d.layer_count() - 1,
                    InsertPosition::Top,
                    LIMIT,
                    0,
                    None,
                )
                .unwrap()
                .unwrap(),
            )
        }
    }
}

// ----------------------------------------------------------------- plan --

/// The kernel's own figure, restated from `remove_model::scratch_bytes`'s
/// documented terms.
fn kernel(w: u32, h: u32, bits: u32, new_layer: bool) -> u64 {
    let cells = u64::from(SIDE * SIDE);
    let n = u64::from(w) * u64::from(h);
    let bpp = if bits == 8 { 4 } else { 8 };
    28 * cells
        + 33 * cells
        + 2 * n * bpp
        + if new_layer { n * bpp } else { 0 }
        + 1024 * u64::from(SIDE + w + h)
}

#[test]
fn the_plan_per_mode_is_its_formula() {
    let (w, h) = (80u32, 60u32);
    let n = u64::from(w * h);
    for depth in DEPTHS {
        let doc = with_adjustment(&build(depth, w, h, &[uniform(w, h, [0.5, 0.4, 0.3, 1.0])]));
        let current = plan(&doc, 0, "current", false).unwrap();
        let below = plan(&doc, 0, "current_and_below", false).unwrap();
        let all = plan(&doc, 0, "all", false).unwrap();
        assert!(current < below, "{depth:?}: {current} < {below}");
        match &doc {
            Doc::Byte(_) => {
                assert_eq!(current, kernel(w, h, 8, false) + 4 * n);
                assert_eq!(plan(&doc, 0, "current", true).unwrap(), current + 4 * n);
                assert_eq!(below, all, "one plate bound covers every composite mode");
            }
            Doc::Typed(d) => {
                for (mode, got) in [
                    (RetouchSampling::Current, current),
                    (RetouchSampling::CurrentAndBelow, below),
                    (RetouchSampling::All, all),
                ] {
                    assert_eq!(
                        got,
                        kernel(w, h, 16, false) + d.retouch_source_plan(0, mode).unwrap()
                    );
                }
                assert_eq!(plan(&doc, 0, "current", true).unwrap(), current + 8 * n);
            }
        }
    }
}

#[test]
fn ignoring_adjustments_drops_them_from_the_all_plate() {
    let (w, h) = (80u32, 60u32);
    let coverage = disc(w, h, (40.0, 30.0), 7.0, 65535);
    for depth in DEPTHS {
        let colour = [0.2, 0.4, 0.3].map(|v| stored(depth, v));
        let doc = with_adjustment(&build(
            depth,
            w,
            h,
            &[uniform(w, h, [colour[0], colour[1], colour[2], 1.0])],
        ));
        let plain = input_with(&doc, 0, "all", true, &coverage, SIDE, LIMIT, ptr::null()).unwrap();
        let adjusted =
            input_with(&doc, 0, "all", false, &coverage, SIDE, LIMIT, ptr::null()).unwrap();
        let cells = (SIDE * SIDE) as usize;
        let k = 0; // a corner cell, far from the hole
        assert_eq!(plain.mask[k], 0.0);
        for c in 0..3 {
            assert!((f64::from(plain.image[c * cells + k]) - colour[c]).abs() <= 1e-5);
            let got = f64::from(adjusted.image[c * cells + k]);
            assert!(
                (got - colour[c]).abs() > 0.01,
                "{depth:?}: the adjustment is sampled"
            );
            if depth == Depth::Byte {
                assert!((got - (1.0 - colour[c])).abs() <= 1e-5, "invert");
            }
        }
    }
}
