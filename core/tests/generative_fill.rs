//! Generative Fill (M3b) through the FFI: `dr_gen_fill_frame`,
//! `dr_doc_gen_fill_*` and `dr_precision_doc_gen_fill_*` at 8 and 16 bits,
//! with a STUB answer in place of the cloud provider.
//!
//! # The oracles
//!
//! Every figure is written here from the header's contract: the native size
//! (least `|ln ratio|` of aspects, the first winning a tie); the crop rule
//! (`k = max(1, 2w/nw, 2h/nh)`, `k' = max(w/nw, h/nh, min(k, W/nw, H/nh))`,
//! centred and slid inside the canvas); the
//! native image (the area-weighted, alpha-weighted mean of the composite, by
//! brute force over each cell's fractional footprint) and mask (any covered
//! pixel in the cell's footprint → (255, 255, 255, 0), else (0, 0, 0, 255));
//! the read-back (a solid answer is the answer; a 2 × 2 answer is the
//! bilinear interpolation between its four pixel centres, clamped at the
//! edges); the mask (the coverage, rounded to bytes at 8 bits).
//!
//! # Bounds, declared before the first run
//!
//! * Native image bytes: ±1 code (an f32 mean, then one rounding).
//! * Native mask bytes, the layer mask, a solid answer at 8 bits and every
//!   pixel outside the selection: exact.
//! * A solid answer at 16 bits and the bilinear read-back: ±1 code.
#![allow(clippy::needless_range_loop)]

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::{DrDocument, LayerKind};
use doctored_core::ffi_color::dr_doc_assign_profile;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_gen_fill::*;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_output_color::dr_doc_assign_profile_refusal;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_color::dr_precision_doc_assign_profile;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use serde_json::json;

use crate::cmyk_fixture::press_profile;
use crate::common::{flat_pixels, layer_bytes, take_err_string};
use crate::float_fixture::float_doc;
use crate::icc_lut_builders::{build as build_profile, tag_desc, tag_wtpt};
use crate::sky_replacement::{build, layer_count, Depth, Doc, LIMIT};

const DEPTHS: [Depth; 2] = [Depth::Byte, Depth::Sixteen];
const OPENAI: [(u32, u32); 3] = [(1024, 1024), (1536, 1024), (1024, 1536)];
const STUB: [u8; 3] = [37, 140, 201];
const TAG: &str = r#"{"type":"generated","version":1,"feature":"generative_fill","provider":"openai","model":"gpt-image-2.5-sunburst","prompt":"a red boat"}"#;

// ---------------------------------------------------------------- calls --

pub(crate) fn frame(
    coverage: &[u16],
    w: u32,
    h: u32,
    sizes: &[(u32, u32)],
) -> Result<[u32; 6], String> {
    let flat: Vec<u32> = sizes.iter().flat_map(|&(a, b)| [a, b]).collect();
    let mut out = [0u32; 6];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_gen_fill_frame(
            coverage.as_ptr(),
            coverage.len(),
            w,
            h,
            flat.as_ptr(),
            flat.len(),
            out.as_mut_ptr(),
            &mut err,
        )
    };
    if ok {
        assert!(err.is_null());
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

fn plan(doc: &Doc, native: (u32, u32)) -> Result<u64, String> {
    let mut err = ptr::null_mut();
    let bytes = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_gen_fill_plan(d, native.0, native.1, 1, &mut err),
            Doc::Typed(d) => dr_precision_doc_gen_fill_plan(d, native.0, native.1, 1, &mut err),
        }
    };
    if bytes == 0 {
        Err(take_err_string(err))
    } else {
        assert!(err.is_null());
        Ok(bytes)
    }
}

pub(crate) struct Input {
    pub(crate) crop: [u32; 4],
    pub(crate) image: Vec<u8>,
    pub(crate) mask: Vec<u8>,
}

pub(crate) fn input_with(
    doc: &Doc,
    layer: usize,
    coverage: &[u16],
    native: (u32, u32),
    limit: u64,
    control: *const DrJobControl,
) -> Result<Input, String> {
    let bytes = (4 * native.0 * native.1) as usize;
    let mut crop = [0u32; 4];
    let mut image = vec![7u8; bytes];
    let mut mask = vec![7u8; bytes];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_gen_fill_input(
                d,
                layer,
                coverage.as_ptr(),
                coverage.len(),
                native.0,
                native.1,
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
            Doc::Typed(d) => dr_precision_doc_gen_fill_input(
                d,
                layer,
                coverage.as_ptr(),
                coverage.len(),
                native.0,
                native.1,
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

fn input(doc: &Doc, layer: usize, coverage: &[u16], native: (u32, u32)) -> Result<Input, String> {
    input_with(doc, layer, coverage, native, LIMIT, ptr::null())
}

struct Answer<'a> {
    pixels: &'a [u8],
    size: (u32, u32),
}

#[allow(clippy::too_many_arguments)]
fn apply_with(
    doc: &Doc,
    layer: usize,
    coverage: &[u16],
    native: (u32, u32),
    answer: &Answer<'_>,
    name: &str,
    tag: &str,
    limit: u64,
    control: *const DrJobControl,
) -> Result<(Doc, usize), String> {
    let name = CString::new(name).unwrap();
    let tag = CString::new(tag).unwrap();
    let sizes = [answer.size.0, answer.size.1];
    let mut index = usize::MAX;
    let mut err = ptr::null_mut();
    let out = unsafe {
        match doc {
            Doc::Byte(d) => {
                let out = dr_doc_gen_fill_apply(
                    d,
                    layer,
                    coverage.as_ptr(),
                    coverage.len(),
                    native.0,
                    native.1,
                    answer.pixels.as_ptr(),
                    answer.pixels.len(),
                    sizes.as_ptr(),
                    sizes.len(),
                    name.as_ptr(),
                    tag.as_ptr(),
                    &mut index,
                    limit,
                    0,
                    control,
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Byte(*Box::from_raw(out)))
            }
            Doc::Typed(d) => {
                let out = dr_precision_doc_gen_fill_apply(
                    d,
                    layer,
                    coverage.as_ptr(),
                    coverage.len(),
                    native.0,
                    native.1,
                    answer.pixels.as_ptr(),
                    answer.pixels.len(),
                    sizes.as_ptr(),
                    sizes.len(),
                    name.as_ptr(),
                    tag.as_ptr(),
                    &mut index,
                    limit,
                    0,
                    control,
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Typed(*Box::from_raw(out)))
            }
        }
    };
    match out {
        Some(doc) => {
            assert!(err.is_null());
            Ok((doc, index))
        }
        None => Err(take_err_string(err)),
    }
}

fn apply(
    doc: &Doc,
    layer: usize,
    coverage: &[u16],
    native: (u32, u32),
    answer: &Answer<'_>,
) -> Result<(Doc, usize), String> {
    apply_with(
        doc,
        layer,
        coverage,
        native,
        answer,
        "a red boat",
        TAG,
        LIMIT,
        ptr::null(),
    )
}

// -------------------------------------------------------------- helpers --

fn stored(depth: Depth, v: f64) -> f64 {
    (v.clamp(0.0, 1.0) * depth.max()).round() / depth.max()
}

fn uniform(w: u32, h: u32, pixel: [f64; 4]) -> Vec<[f64; 4]> {
    vec![pixel; (w * h) as usize]
}

pub(crate) fn gradient(depth: Depth, w: u32, h: u32) -> Vec<[f64; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (f64::from(i % w), f64::from(i / w));
            [
                x / f64::from(w),
                y / f64::from(h),
                ((x + y) % 13.0) / 13.0,
                1.0,
            ]
            .map(|v| stored(depth, v))
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

/// A full disc of radius `r` with a soft ring out to `r + 4` at 20000.
fn soft_disc(w: u32, h: u32, (cx, cy): (f64, f64), r: f64) -> Vec<u16> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (f64::from(i % w) + 0.5, f64::from(i / w) + 0.5);
            let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            if d <= r {
                65535
            } else if d <= r + 4.0 {
                20000
            } else {
                0
            }
        })
        .collect()
}

/// The covered rect of `coverage`, `(x, y, w, h)`.
fn covered(coverage: &[u16], w: u32) -> (u32, u32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, c) in coverage.iter().enumerate() {
        if *c > 0 {
            let (x, y) = (i as u32 % w, i as u32 / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    (x0, y0, x1 - x0, y1 - y0)
}

/// The crop rule, restated from the header.
fn crop_oracle(rect: (u32, u32, u32, u32), canvas: (u32, u32), native: (u32, u32)) -> [u32; 4] {
    let (x0, y0, w, h) = rect;
    let (nw, nh) = (f64::from(native.0), f64::from(native.1));
    let k = [1.0, 2.0 * f64::from(w) / nw, 2.0 * f64::from(h) / nh]
        .into_iter()
        .fold(0.0, f64::max);
    let fits = [k, f64::from(canvas.0) / nw, f64::from(canvas.1) / nh]
        .into_iter()
        .fold(f64::INFINITY, f64::min);
    let k = [f64::from(w) / nw, f64::from(h) / nh, fits]
        .into_iter()
        .fold(0.0, f64::max);
    let cw = ((k * nw).ceil() as u32).min(canvas.0);
    let ch = ((k * nh).ceil() as u32).min(canvas.1);
    let x =
        (i64::from(x0) + i64::from(w / 2) - i64::from(cw / 2)).clamp(0, i64::from(canvas.0 - cw));
    let y =
        (i64::from(y0) + i64::from(h / 2) - i64::from(ch / 2)).clamp(0, i64::from(canvas.1 - ch));
    [x as u32, y as u32, cw, ch]
}

/// The native size the header names: least `|ln ratio|`, the first on a tie.
fn native_oracle(rect: (u32, u32, u32, u32), sizes: &[(u32, u32)]) -> (u32, u32) {
    let aspect = f64::from(rect.2) / f64::from(rect.3);
    let mut best = sizes[0];
    let mut score = f64::INFINITY;
    for &(a, b) in sizes {
        let s = (f64::from(a) / f64::from(b) / aspect).ln().abs();
        if s < score {
            score = s;
            best = (a, b);
        }
    }
    best
}

/// The native cells of `picture` (unit, `w` wide) and `coverage` over
/// `crop`, by brute force over each cell's fractional footprint.
pub(crate) fn cells_oracle(
    picture: &[[f64; 4]],
    coverage: &[u16],
    w: u32,
    crop: [u32; 4],
    native: (u32, u32),
) -> (Vec<[f64; 3]>, Vec<bool>) {
    let [x, y, cw, ch] = crop.map(f64::from);
    let (nw, nh) = native;
    let mut image = vec![[0.0; 3]; (nw * nh) as usize];
    let mut mask = vec![false; (nw * nh) as usize];
    for j in 0..nh {
        for i in 0..nw {
            let (ax, bx) = (
                f64::from(i) * cw / f64::from(nw),
                f64::from(i + 1) * cw / f64::from(nw),
            );
            let (ay, by) = (
                f64::from(j) * ch / f64::from(nh),
                f64::from(j + 1) * ch / f64::from(nh),
            );
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
            let k = (j * nw + i) as usize;
            image[k] = if weight > 0.0 {
                sum.map(|v| v / weight)
            } else {
                [0.0; 3]
            };
            mask[k] = any;
        }
    }
    (image, mask)
}

fn solid(size: (u32, u32), rgb: [u8; 3]) -> Vec<u8> {
    (0..size.0 * size.1)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect()
}

/// The projection as unit samples.
pub(crate) fn projection(doc: &Doc) -> Vec<f64> {
    match doc {
        Doc::Byte(d) => flat_pixels(d as *const DrDocument)
            .iter()
            .map(|&v| f64::from(v) / 255.0)
            .collect(),
        Doc::Typed(d) => d
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap()
            .iter()
            .flat_map(|p| p.map(|v| f64::from(v) / 65535.0))
            .collect(),
    }
}

/// Layer `idx`'s own samples as unit floats.
fn unit_layer(doc: &Doc, idx: usize) -> Vec<[f64; 4]> {
    match doc {
        Doc::Byte(d) => layer_bytes(d, idx)
            .chunks(4)
            .map(|p| [0, 1, 2, 3].map(|c| f64::from(p[c]) / 255.0))
            .collect(),
        Doc::Typed(d) => d
            .layer(idx)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| p.map(|v| f64::from(v) / 65535.0))
            .collect(),
    }
}

fn layer_name(doc: &Doc, idx: usize) -> String {
    match doc {
        Doc::Byte(d) => d.layers[idx].name.clone(),
        Doc::Typed(d) => d.layer(idx).unwrap().name().to_string(),
    }
}

fn layer_tag(doc: &Doc, idx: usize) -> Option<String> {
    match doc {
        Doc::Byte(d) => d.layers[idx].meta.clone(),
        Doc::Typed(d) => d.artwork_description(idx).map(str::to_string),
    }
}

/// The layer mask as unit samples, and whether it is enabled.
fn layer_mask(doc: &Doc, idx: usize) -> (Vec<f64>, bool) {
    match doc {
        Doc::Byte(d) => {
            let layer = &d.layers[idx];
            let mask = layer.mask.as_ref().expect("a mask");
            (
                mask.as_raw()
                    .iter()
                    .map(|&v| f64::from(v) / 255.0)
                    .collect(),
                layer.mask_enabled,
            )
        }
        Doc::Typed(d) => {
            let mask = d.layer(idx).unwrap().mask().expect("a mask");
            (
                mask.plane()
                    .samples()
                    .iter()
                    .map(|&v| f64::from(v) / 65535.0)
                    .collect(),
                mask.enabled(),
            )
        }
    }
}

fn assert_codes(depth: Depth, got: f64, want: f64, what: &str) {
    let diff = (got - want).abs() * depth.max();
    assert!(
        diff <= 1.0 + 1e-6,
        "{what} at {depth:?}: got {got}, want {want} ({diff} codes)"
    );
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

/// `doc` with layer 0 in a group, and the group's index.
fn with_group(doc: &Doc) -> (Doc, usize) {
    match doc {
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
    }
}

// ------------------------------------------------------------- framing --

#[test]
fn the_native_size_follows_the_aspect_and_the_crop_its_rule() {
    let (w, h) = (3000u32, 2000u32);
    let cases = [
        (100, 100, 600, 100),   // wide
        (100, 100, 100, 600),   // tall
        (1400, 900, 200, 200),  // square
        (0, 0, 40, 30),         // a corner, slid inside
        (2900, 1950, 100, 50),  // the far corner
        (200, 100, 2600, 1800), // larger than half the canvas: clipped
    ];
    for (x, y, cw, ch) in cases {
        let coverage = rect_coverage(w, h, (x, y, x + cw, y + ch));
        let got = frame(&coverage, w, h, &OPENAI).unwrap();
        let native = native_oracle((x, y, cw, ch), &OPENAI);
        assert_eq!((got[4], got[5]), native, "native for {x},{y} {cw}×{ch}");
        let crop = crop_oracle((x, y, cw, ch), (w, h), native);
        assert_eq!(got[..4], crop, "crop for {x},{y} {cw}×{ch}");
        // The crop holds the covered rect and lies inside the canvas.
        assert!(crop[0] <= x && crop[1] <= y);
        assert!(crop[0] + crop[2] >= x + cw && crop[1] + crop[3] >= y + ch);
        assert!(crop[0] + crop[2] <= w && crop[1] + crop[3] <= h);
    }
    let wide = rect_coverage(w, h, (100, 100, 700, 200));
    assert_eq!(frame(&wide, w, h, &OPENAI).unwrap()[4..], [1536, 1024]);
    let tall = rect_coverage(w, h, (100, 100, 200, 700));
    assert_eq!(frame(&tall, w, h, &OPENAI).unwrap()[4..], [1024, 1536]);
    // A tie goes to the first size.
    let square = rect_coverage(w, h, (100, 100, 200, 200));
    assert_eq!(
        frame(&square, w, h, &[(64, 64), (128, 128)]).unwrap()[4..],
        [64, 64]
    );
    // A small canvas: the crop is the largest square the canvas holds,
    // resampled up.
    let small = rect_coverage(60, 40, (10, 10, 20, 20));
    let got = frame(&small, 60, 40, &OPENAI).unwrap();
    assert_eq!(got[4..], [1024, 1024]);
    assert_eq!(
        got[..4],
        crop_oracle((10, 10, 10, 10), (60, 40), (1024, 1024))
    );
    assert_eq!(got[..4], [0, 0, 40, 40]);
    // A tall rect on the right edge of a short canvas (Generative Expand's
    // frame): the crop keeps the native aspect rather than clipping one axis.
    let (ew, eh) = (5000u32, 3000u32);
    let edge = rect_coverage(ew, eh, (4000, 0, 5000, 3000));
    let got = frame(&edge, ew, eh, &OPENAI).unwrap();
    assert_eq!(got[4..], [1024, 1536]);
    let crop = crop_oracle((4000, 0, 1000, 3000), (ew, eh), (1024, 1536));
    assert_eq!(got[..4], crop);
    assert_eq!(crop, [3000, 0, 2000, 3000]);
    assert_eq!(u64::from(crop[2]) * 1536, u64::from(crop[3]) * 1024);
    // Refusals.
    assert_eq!(
        frame(&vec![0; (w * h) as usize], w, h, &OPENAI).unwrap_err(),
        "the selection covers no pixel of the picture"
    );
    assert_eq!(
        frame(&wide, w, h, &[(32, 64)]).unwrap_err(),
        "generative fill native size must be 64…2048 on each side"
    );
    assert_eq!(
        frame(&wide, w, h, &[]).unwrap_err(),
        "invalid generative fill native sizes"
    );
    assert_eq!(
        frame(&wide[1..], w, h, &OPENAI).unwrap_err(),
        "invalid generative fill coverage buffer"
    );
}

// --------------------------------------------------------------- input --

#[test]
fn the_input_cells_are_the_area_weighted_mean_and_the_mask_its_rule() {
    for depth in DEPTHS {
        for ((w, h), hole, native) in [
            (
                (300u32, 200u32),
                (100u32, 80u32, 190u32, 120u32),
                (96u32, 64u32),
            ),
            ((120, 100), (40, 30, 60, 75), (64, 96)),
            ((50, 40), (10, 8, 18, 14), (64, 64)),
        ] {
            // A translucent picture, so the mean is alpha-weighted.
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
                    .map(|v| stored(depth, v))
                })
                .collect();
            let doc = build(depth, w, h, std::slice::from_ref(&picture));
            let coverage = rect_coverage(w, h, hole);
            let got = input(&doc, 0, &coverage, native).unwrap();
            let rect = covered(&coverage, w);
            assert_eq!(got.crop, crop_oracle(rect, (w, h), native));
            let (image, mask) = cells_oracle(&picture, &coverage, w, got.crop, native);
            for k in 0..(native.0 * native.1) as usize {
                let want_mask: [u8; 4] = if mask[k] {
                    [255, 255, 255, 0]
                } else {
                    [0, 0, 0, 255]
                };
                assert_eq!(
                    got.mask[4 * k..4 * k + 4],
                    want_mask,
                    "{depth:?} mask cell {k}"
                );
                let want_alpha = if mask[k] { 0 } else { 255 };
                assert_eq!(
                    got.image[4 * k + 3],
                    want_alpha,
                    "{depth:?} image alpha {k}"
                );
                for c in 0..3 {
                    let want = image[k][c] * 255.0;
                    let diff = (f64::from(got.image[4 * k + c]) - want).abs();
                    assert!(diff <= 1.0, "{depth:?} cell {k} channel {c}: {diff}");
                }
            }
        }
    }
}

#[test]
fn the_input_reads_the_full_composite() {
    let (w, h) = (80u32, 60u32);
    let native = (64, 64);
    for depth in DEPTHS {
        let colour = [0.2, 0.4, 0.3].map(|v| stored(depth, v));
        let base = build(
            depth,
            w,
            h,
            &[uniform(w, h, [colour[0], colour[1], colour[2], 1.0])],
        );
        let doc = with_adjustment(&base);
        let coverage = rect_coverage(w, h, (30, 20, 40, 30));
        // Anchored at the bottom layer, the adjustment above it is sampled.
        let got = input(&doc, 0, &coverage, native).unwrap();
        let k = 0; // a corner cell, far from the hole
        assert_eq!(got.mask[4 * k + 3], 255);
        for c in 0..3 {
            let v = f64::from(got.image[4 * k + c]) / 255.0;
            assert!(
                (v - colour[c]).abs() > 0.01,
                "{depth:?}: the adjustment is sampled"
            );
            if depth == Depth::Byte {
                assert!((v - (1.0 - colour[c])).abs() <= 1.0 / 255.0, "invert");
            }
        }
    }
}

// --------------------------------------------------------------- apply --

#[test]
fn a_solid_answer_lands_on_a_new_masked_layer_and_nothing_else_moves() {
    let (w, h) = (160u32, 120u32);
    let native = (96, 64);
    for depth in DEPTHS {
        let doc = build(
            depth,
            w,
            h,
            &[gradient(depth, w, h), uniform(w, h, [0.0; 4])],
        );
        let before = projection(&doc);
        let coverage = soft_disc(w, h, (80.0, 60.0), 12.0);
        let answer = solid(native, STUB);
        let (out, index) = apply(
            &doc,
            0,
            &coverage,
            native,
            &Answer {
                pixels: &answer,
                size: native,
            },
        )
        .unwrap();
        assert_eq!(index, 1, "{depth:?}: directly above the anchor");
        assert_eq!(layer_count(&out), 3);
        assert_eq!(layer_name(&out, index), "a red boat");
        assert_eq!(layer_tag(&out, index).as_deref(), Some(TAG));
        // The mask is the coverage (rounded to bytes at 8 bits).
        let (mask, enabled) = layer_mask(&out, index);
        assert!(enabled);
        for i in 0..(w * h) as usize {
            let c = f64::from(coverage[i]) / 65535.0;
            let want = match depth {
                Depth::Byte => (c * 255.0).round() / 255.0,
                Depth::Sixteen => c,
            };
            assert_eq!(mask[i], want, "{depth:?} mask {i}");
        }
        // The layer: opaque stub on the covered rect, transparent elsewhere.
        let rect = covered(&coverage, w);
        let pixels = unit_layer(&out, index);
        for i in 0..(w * h) as usize {
            let (x, y) = (i as u32 % w, i as u32 / w);
            let inside =
                (rect.0..rect.0 + rect.2).contains(&x) && (rect.1..rect.1 + rect.3).contains(&y);
            if !inside {
                assert_eq!(pixels[i], [0.0; 4], "{depth:?} outside the rect {i}");
                continue;
            }
            assert_eq!(pixels[i][3], 1.0);
            for c in 0..3 {
                assert_codes(depth, pixels[i][c], f64::from(STUB[c]) / 255.0, "layer");
            }
        }
        // The composite: unchanged outside the selection, the stub where it
        // is fully covered.
        let after = projection(&out);
        for i in 0..(w * h) as usize {
            let px = &after[4 * i..4 * i + 4];
            if coverage[i] == 0 {
                assert_eq!(px, &before[4 * i..4 * i + 4], "{depth:?} pixel {i} moved");
            } else if coverage[i] == 65535 {
                for c in 0..3 {
                    let want = f64::from(STUB[c]) / 255.0;
                    match depth {
                        Depth::Byte => assert_eq!(px[c], want, "pixel {i}"),
                        Depth::Sixteen => assert_codes(depth, px[c], want, "composite"),
                    }
                }
                assert_eq!(px[3], 1.0);
            }
        }
        // The anchor and the layer above it are untouched.
        assert_eq!(unit_layer(&out, 0), unit_layer(&doc, 0));
        assert_eq!(unit_layer(&out, 2), unit_layer(&doc, 1));
    }
}

#[test]
fn a_two_by_two_answer_is_read_back_bilinearly() {
    let (w, h) = (200u32, 150u32);
    let native = (64, 64);
    let corners: [[u8; 3]; 4] = [[0, 0, 0], [255, 0, 40], [0, 255, 80], [255, 255, 120]];
    let answer: Vec<u8> = corners
        .iter()
        .flat_map(|c| [c[0], c[1], c[2], 255])
        .collect();
    for depth in DEPTHS {
        let doc = build(depth, w, h, &[gradient(depth, w, h)]);
        let coverage = rect_coverage(w, h, (70, 50, 130, 100));
        let crop = input(&doc, 0, &coverage, native).unwrap().crop;
        let (out, index) = apply(
            &doc,
            0,
            &coverage,
            native,
            &Answer {
                pixels: &answer,
                size: (2, 2),
            },
        )
        .unwrap();
        let pixels = unit_layer(&out, index);
        let [cx, cy, cw, ch] = crop.map(f64::from);
        for y in 50..100u32 {
            for x in 70..130u32 {
                let qx = (f64::from(x) + 0.5 - cx) * 2.0 / cw;
                let qy = (f64::from(y) + 0.5 - cy) * 2.0 / ch;
                let (fx, fy) = ((qx - 0.5).clamp(0.0, 1.0), (qy - 0.5).clamp(0.0, 1.0));
                let p = pixels[(y * w + x) as usize];
                for c in 0..3 {
                    let v = |k: usize| f64::from(corners[k][c]) / 255.0;
                    let want = (1.0 - fx) * (1.0 - fy) * v(0)
                        + fx * (1.0 - fy) * v(1)
                        + (1.0 - fx) * fy * v(2)
                        + fx * fy * v(3);
                    assert_codes(depth, p[c], want, "bilinear");
                }
                assert_eq!(p[3], 1.0);
            }
        }
    }
}

#[test]
fn the_tag_rides_promotion_to_16_bits() {
    let (w, h) = (64u32, 48u32);
    let doc = build(Depth::Byte, w, h, &[gradient(Depth::Byte, w, h)]);
    let coverage = rect_coverage(w, h, (10, 10, 30, 30));
    let answer = solid((64, 64), STUB);
    let (out, index) = apply(
        &doc,
        0,
        &coverage,
        (64, 64),
        &Answer {
            pixels: &answer,
            size: (64, 64),
        },
    )
    .unwrap();
    let Doc::Byte(byte) = out else { unreachable!() };
    let mut err = ptr::null_mut();
    let typed = unsafe { dr_doc_promote_structure(&byte, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!typed.is_null(), "{}", take_err_string(err));
    let typed = unsafe { Box::from_raw(typed) };
    assert_eq!(typed.artwork_description(index), Some(TAG));
    assert!(typed.layer(index).unwrap().mask().is_some());
}

#[test]
fn any_anchor_kind_works_at_both_depths() {
    let (w, h) = (80u32, 60u32);
    let native = (64, 64);
    let coverage = rect_coverage(w, h, (20, 20, 40, 40));
    let answer = solid(native, STUB);
    let answer = Answer {
        pixels: &answer,
        size: native,
    };
    for depth in DEPTHS {
        let doc = build(depth, w, h, &[gradient(depth, w, h)]);
        let adjusted = with_adjustment(&doc);
        let top = layer_count(&adjusted) - 1;
        assert!(plan(&adjusted, native).is_ok());
        assert!(
            input(&adjusted, top, &coverage, native).is_ok(),
            "{depth:?}"
        );
        let (out, index) = apply(&adjusted, top, &coverage, native, &answer).unwrap();
        assert_eq!(index, top + 1, "{depth:?}: above the adjustment");
        assert_eq!(layer_tag(&out, index).as_deref(), Some(TAG));
        let (grouped, group) = with_group(&doc);
        assert!(
            input(&grouped, group, &coverage, native).is_ok(),
            "{depth:?}"
        );
        let (out, index) = apply(&grouped, group, &coverage, native, &answer).unwrap();
        assert_eq!(index, group + 1, "{depth:?}: above the group");
        assert_eq!(layer_name(&out, index), "a red boat");
    }
}

// --------------------------------------------------------------- assign --

/// An RGB matrix/TRC profile whose three TRCs are a count-1 `curv` of gamma
/// 0. A copy of `encode_refusals::bad_profile` (the master), which lives in
/// another suite binary.
fn bad_profile() -> Vec<u8> {
    let mut curve = b"curv\0\0\0\0".to_vec();
    curve.extend_from_slice(&1u32.to_be_bytes());
    curve.extend_from_slice(&0u16.to_be_bytes());
    let xyz = |v: [f64; 3]| tag_wtpt(v);
    build_profile(
        [0x02, 0x10, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic Bad Gamma")),
            (b"rXYZ", xyz([0.4361, 0.2225, 0.0139])),
            (b"gXYZ", xyz([0.3851, 0.7169, 0.0971])),
            (b"bXYZ", xyz([0.1431, 0.0606, 0.7141])),
            (b"rTRC", curve.clone()),
            (b"gTRC", curve.clone()),
            (b"bTRC", curve),
        ],
    )
}

#[test]
fn a_generated_layer_does_not_refuse_assign_as_artwork() {
    let (w, h) = (32u32, 24u32);
    let coverage = rect_coverage(w, h, (4, 4, 20, 16));
    let answer = solid((64, 64), STUB);
    let answer = Answer {
        pixels: &answer,
        size: (64, 64),
    };
    let bad = bad_profile();
    for depth in DEPTHS {
        let doc = build(depth, w, h, &[gradient(depth, w, h)]);
        let (out, _) = apply(&doc, 0, &coverage, (64, 64), &answer).unwrap();
        let mut err = ptr::null_mut();
        match &out {
            Doc::Byte(d) => {
                let refusal = unsafe { dr_doc_assign_profile_refusal(d, bad.as_ptr(), bad.len()) };
                assert!(refusal.is_null(), "{}", take_err_string(refusal));
                let assigned =
                    unsafe { dr_doc_assign_profile(d, bad.as_ptr(), bad.len(), &mut err) };
                assert!(!assigned.is_null(), "{}", take_err_string(err));
                unsafe { dr_doc_free(assigned) };
            }
            Doc::Typed(d) => {
                let assigned = unsafe {
                    dr_precision_doc_assign_profile(
                        d,
                        bad.as_ptr(),
                        bad.len(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                };
                assert!(!assigned.is_null(), "{}", take_err_string(err));
                unsafe { dr_precision_doc_free(assigned) };
            }
        }
    }
}

// ------------------------------------------------------------- refusals --

#[test]
fn every_refusal_names_itself() {
    let (w, h) = (80u32, 60u32);
    let native = (64, 64);
    let coverage = rect_coverage(w, h, (20, 20, 40, 40));
    let pixels = solid(native, STUB);
    let answer = Answer {
        pixels: &pixels,
        size: native,
    };
    for depth in DEPTHS {
        let doc = build(depth, w, h, &[gradient(depth, w, h)]);
        let range = "generative fill layer 5 is out of range";
        assert_eq!(input(&doc, 5, &coverage, native).err().unwrap(), range);
        assert_eq!(
            apply(&doc, 5, &coverage, native, &answer).err().unwrap(),
            range
        );
        let side = "generative fill native size must be 64…2048 on each side";
        assert_eq!(plan(&doc, (32, 64)).unwrap_err(), side);
        assert_eq!(input(&doc, 0, &coverage, (64, 4096)).err().unwrap(), side);
        assert_eq!(
            apply(&doc, 0, &coverage, (4096, 64), &answer)
                .err()
                .unwrap(),
            side
        );
        let buffer = "invalid generative fill coverage buffer";
        assert_eq!(
            input(&doc, 0, &coverage[1..], native).err().unwrap(),
            buffer
        );
        assert_eq!(
            apply(&doc, 0, &coverage[1..], native, &answer)
                .err()
                .unwrap(),
            buffer
        );
        let empty = vec![0u16; coverage.len()];
        let nothing = "the selection covers no pixel of the picture";
        assert_eq!(input(&doc, 0, &empty, native).err().unwrap(), nothing);
        assert_eq!(
            apply(&doc, 0, &empty, native, &answer).err().unwrap(),
            nothing
        );
        let short = Answer {
            pixels: &pixels[4..],
            size: native,
        };
        assert_eq!(
            apply(&doc, 0, &coverage, native, &short).err().unwrap(),
            "invalid generative fill result buffer"
        );
        let huge = vec![0u8; 4 * 4097];
        let wide = Answer {
            pixels: &huge,
            size: (4097, 1),
        };
        assert_eq!(
            apply(&doc, 0, &coverage, native, &wide).err().unwrap(),
            "generative fill result must be 1…4096 on each side"
        );
        let unnamed = apply_with(
            &doc,
            0,
            &coverage,
            native,
            &answer,
            "",
            TAG,
            LIMIT,
            ptr::null(),
        );
        assert_eq!(unnamed.err().unwrap(), "generative fill needs a layer name");
        for tag in [
            r#"{"type":"text","version":1}"#,
            r#"{"type":"fill","version":1}"#,
        ] {
            let wrong = apply_with(
                &doc,
                0,
                &coverage,
                native,
                &answer,
                "G",
                tag,
                LIMIT,
                ptr::null(),
            );
            assert_eq!(
                wrong.err().unwrap(),
                "generative fill requires a generated artwork description"
            );
        }
        let unversioned = apply_with(
            &doc,
            0,
            &coverage,
            native,
            &answer,
            "G",
            r#"{"type":"generated","version":0}"#,
            LIMIT,
            ptr::null(),
        );
        assert_eq!(
            unversioned.err().unwrap(),
            "artwork description requires text, shape, fill or generated with a positive version"
        );
        // A working limit below the plan, and a cancelled control.
        assert!(input_with(&doc, 0, &coverage, native, 4096, ptr::null())
            .err()
            .unwrap()
            .contains("working limit"));
        assert!(apply_with(
            &doc,
            0,
            &coverage,
            native,
            &answer,
            "G",
            TAG,
            4096,
            ptr::null()
        )
        .err()
        .unwrap()
        .contains("working limit"));
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let cancelled = input_with(&doc, 0, &coverage, native, LIMIT, control);
        unsafe { dr_job_control_free(control) };
        assert_eq!(cancelled.err().as_deref(), Some("operation cancelled"));
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let cancelled = apply_with(
            &doc, 0, &coverage, native, &answer, "G", TAG, LIMIT, control,
        );
        unsafe { dr_job_control_free(control) };
        assert_eq!(cancelled.err().as_deref(), Some("operation cancelled"));
    }
    // 32-bit float and 16-bit CMYK refuse by name at every export.
    let n = (w * h) as usize;
    let float = Doc::Typed(float_doc(
        w,
        h,
        &IccProfile::srgb(),
        &[vec![[0.5, 0.5, 0.5, 1.0]; n]],
    ));
    let not_float = "generative fill is not available in a 32-bit float document";
    assert_eq!(plan(&float, native).unwrap_err(), not_float);
    assert_eq!(
        input(&float, 0, &coverage, native).err().unwrap(),
        not_float
    );
    assert_eq!(
        apply(&float, 0, &coverage, native, &answer).err().unwrap(),
        not_float
    );
    let rgb = match build(Depth::Sixteen, w, h, &[uniform(w, h, [0.5, 0.4, 0.3, 1.0])]) {
        Doc::Typed(d) => Box::into_raw(Box::new(d)),
        Doc::Byte(_) => unreachable!(),
    };
    let profile = press_profile("Press Generative Fill");
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
        doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk(
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
    let not_cmyk = "generative fill is not available in a 16-bit CMYK document";
    assert_eq!(plan(&cmyk_doc, native).unwrap_err(), not_cmyk);
    assert_eq!(
        input(&cmyk_doc, 0, &coverage, native).err().unwrap(),
        not_cmyk
    );
    assert_eq!(
        apply(&cmyk_doc, 0, &coverage, native, &answer)
            .err()
            .unwrap(),
        not_cmyk
    );
    unsafe {
        dr_precision_doc_free(cmyk);
        dr_precision_doc_free(rgb);
    }
    // A NULL document carries the standard message.
    let mut err = ptr::null_mut();
    assert_eq!(
        unsafe { dr_doc_gen_fill_plan(ptr::null(), 64, 64, 1, &mut err) },
        0
    );
    assert_eq!(take_err_string(err), "required handle is NULL");
}
