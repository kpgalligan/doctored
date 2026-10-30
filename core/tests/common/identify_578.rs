//! The identify-578 take's pictures, renders and profiles (#578), shared by
//! the Inner Glow and the Bevel & Emboss identify parity tests
//! (`style_inner_glow_identify_parity.rs`, `style_bevel_identify_parity.rs`):
//! each row is rebuilt as `identify_578_take.py` builds it, a black,
//! binary-alpha layer over opaque black, rendered as an 8-bit `DrDocument`
//! or a 16-bit `PrecisionDocument`, and read as the take's profile
//! (`I.profile`) on the red channel in 8-bit codes. The second take's
//! anti-aliased pictures (`style_inner_glow_choke100_parity.rs`) render
//! through [`render8_alpha`] / [`render16_alpha`] on [`supersampled`]
//! coverage, and its Emboss and Pillow Emboss rows
//! (`style_bevel_emboss_take2_parity.rs`) render the first take's binary
//! pictures and read the second take's whole profile ([`profile2`]). The
//! fourth take's anti-aliased Inner Glow rows
//! (`style_inner_glow_aa_parity.rs`) also render at 32 bits
//! ([`render32_alpha`]); its pictures are [`take4_coverage`], which the
//! fourth Stroke take shares (`style_stroke_aa_parity.rs`, coloured layers
//! read on all three channels through [`render_rgb_alpha`]); the Stroke
//! confirm take's pictures are [`take5_coverage`], binned by [`take5_bin`];
//! the effect-kernels-578-p3 identify take's are [`p3_coverage`]
//! (`style_effect_kernels_578_p3_take.rs`), and the effect-kernels-578-p4
//! takes' [`p4_coverage`] (`style_effect_kernels_578_p4_take.rs`).

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// The picture's size and binary alpha (`identify_578_take.alpha_of`).
pub(crate) fn alpha(shape: &str) -> (u32, u32, Vec<bool>) {
    let (w, h) = match shape {
        "half" => (320, 320),
        "disc" => (160, 160),
        "corner" => (224, 224),
        _ => panic!("unknown shape {shape}"),
    };
    let inside = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| match shape {
            "half" => (32..256).contains(&x) && (32..288).contains(&y),
            "disc" => disc_distance(x, y) <= 40.0,
            _ => (32..192).contains(&x) && (32..192).contains(&y),
        })
        .collect();
    (w, h, inside)
}

pub(crate) fn disc_distance(x: u32, y: u32) -> f64 {
    (f64::from(x) + 0.5 - 80.0).hypot(f64::from(y) + 0.5 - 80.0)
}

/// Area coverage of the region `inside(x, y)` (continuous; pixel (i, j)
/// spans [i, i + 1) x [j, j + 1)) on an 8 x 8 sub-grid, row-major, as
/// `style_v1x_take.supersampled` builds the take2 pictures.
pub(crate) fn supersampled(w: u32, h: u32, inside: impl Fn(f64, f64) -> bool) -> Vec<f64> {
    let n = 8u32;
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let mut count = 0u32;
            for sy in 0..n {
                for sx in 0..n {
                    let px = f64::from(x) + (f64::from(sx) + 0.5) / f64::from(n);
                    let py = f64::from(y) + (f64::from(sy) + 0.5) / f64::from(n);
                    count += u32::from(inside(px, py));
                }
            }
            f64::from(count) / f64::from(n * n)
        })
        .collect()
}

/// The 8-bit render's red, in codes.
pub(crate) fn render8(w: u32, h: u32, inside: &[bool], style: &Value) -> Vec<f64> {
    let alpha: Vec<u8> = inside.iter().map(|&i| if i { 255 } else { 0 }).collect();
    render8_alpha(w, h, &alpha, style)
}

/// The 8-bit render's red, in codes, of a black layer with byte `alpha`.
pub(crate) fn render8_alpha(w: u32, h: u32, alpha: &[u8], style: &Value) -> Vec<f64> {
    let source = RgbaImage::from_fn(w, h, |x, y| Rgba([0, 0, 0, alpha[(y * w + x) as usize]]));
    let d = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255])))
        .adding_image_layer(0, source, "fx")
        .expect("layer");
    common::styled(&d, 1, &style.to_string())
        .flattened()
        .expect("flatten")
        .pixels()
        .map(|p| f64::from(p[0]))
        .collect()
}

/// The 16-bit render's red, in 8-bit codes (`/ 257`).
pub(crate) fn render16(w: u32, h: u32, inside: &[bool], style: &Value) -> Vec<f64> {
    let alpha: Vec<u16> = inside.iter().map(|&i| if i { 65535 } else { 0 }).collect();
    render16_alpha(w, h, &alpha, style)
}

/// The 16-bit render's red, in 8-bit codes (`/ 257`), of a black layer with
/// 16-bit `alpha`.
pub(crate) fn render16_alpha(w: u32, h: u32, alpha: &[u16], style: &Value) -> Vec<f64> {
    let image = |pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).expect("image")
    };
    let layer = alpha.iter().map(|&a| [0, 0, 0, a]).collect();
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Base",
                image(vec![[0, 0, 0, 65535]; (w * h) as usize]),
                (0, 0),
                1.0,
                true,
            )
            .expect("base"),
            Layer::raster("fx", image(layer), (0, 0), 1.0, true).expect("fx"),
        ],
    )
    .expect("document");
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", common::take_err_string(err));
    let styled = unsafe { Box::from_raw(out) };
    styled
        .render(LIMIT, 0, None)
        .expect("render")
        .to_rgba16()
        .expect("rgba16")
        .into_iter()
        .map(|p| f64::from(p[0]) / 257.0)
        .collect()
}

/// The 32-bit render's red × 255, of a black layer with float `alpha`:
/// [`render16_alpha`] on a float document of the same two records
/// (`float_fixture::float_doc`; a raster layer takes RGBA16 only).
pub(crate) fn render32_alpha(w: u32, h: u32, alpha: &[f32], style: &Value) -> Vec<f64> {
    let layer = alpha.iter().map(|&a| [0.0, 0.0, 0.0, a]).collect();
    let doc = crate::float_fixture::float_doc(
        w,
        h,
        &IccProfile::srgb(),
        &[vec![[0.0, 0.0, 0.0, 1.0]; (w * h) as usize], layer],
    );
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", common::take_err_string(err));
    let styled = unsafe { Box::from_raw(out) };
    styled
        .render(LIMIT, 0, None)
        .expect("render")
        .to_rgba32()
        .expect("rgba32")
        .into_iter()
        .map(|p| f64::from(p[0]) * 255.0)
        .collect()
}

/// The fourth take's `aahalf` bands: (coverage %, y0, y1) of the x = 256
/// column (`identify_578_take4.BANDS`).
const TAKE4_BANDS: [(u32, u32, u32); 3] = [(25, 32, 192), (50, 192, 352), (75, 352, 512)];

/// The fourth take's picture size and coverage (`identify_578_take4.alpha_of`):
/// `aahalf` 320 × 544, the rect x in [32, 256), y in [32, 512), its x = 256
/// column at 0.25 / 0.5 / 0.75 ([`TAKE4_BANDS`]); `aadisc` 160², the 8 × 8
/// supersampled r = 40 disc at (80, 80).
pub(crate) fn take4_coverage(shape: &str) -> (u32, u32, Vec<f64>) {
    match shape {
        "aahalf" => {
            let (w, h) = (320u32, 544u32);
            let a = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| {
                    if (32..256).contains(&x) && (32..512).contains(&y) {
                        return 1.0;
                    }
                    if x == 256 {
                        for &(c, y0, y1) in &TAKE4_BANDS {
                            if (y0..y1).contains(&y) {
                                return f64::from(c) / 100.0;
                            }
                        }
                    }
                    0.0
                })
                .collect();
            (w, h, a)
        }
        "aadisc" => (
            160,
            160,
            supersampled(160, 160, |x, y| {
                (x - 80.0).powi(2) + (y - 80.0).powi(2) <= 1600.0
            }),
        ),
        _ => panic!("unknown shape {shape}"),
    }
}

/// The effect-kernels-578-p3 identify take's pictures, 256 × 224
/// (`effect_kernels_578_p3_take.picture_alpha`): `hp` the binary rect x in
/// [48, 168), y in [24, 200); `disc` the binary r = 40 disc at (128, 112),
/// tested at each pixel's centre; `ell` the 8 × 8 supersampled ellipse at
/// (128, 112) with semi-axes 76 × 56.
pub(crate) fn p3_coverage(shape: &str) -> (u32, u32, Vec<f64>) {
    let (w, h) = (256u32, 224u32);
    let a = match shape {
        "hp" => (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| f64::from(u8::from((48..168).contains(&x) && (24..200).contains(&y))))
            .collect(),
        "disc" => (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (cx, cy) = (f64::from(x) + 0.5 - 128.0, f64::from(y) + 0.5 - 112.0);
                f64::from(u8::from(cx * cx + cy * cy <= 1600.0))
            })
            .collect(),
        "ell" => supersampled(w, h, |x, y| {
            ((x - 128.0) / 76.0).powi(2) + ((y - 112.0) / 56.0).powi(2) <= 1.0
        }),
        _ => panic!("unknown shape {shape}"),
    };
    (w, h, a)
}

/// The effect-kernels-578-p4 takes' pictures, 256 × 224
/// (`effect_kernels_578_p4_take.picture_alpha`): `disc` and `ell` as
/// [`p3_coverage`]'s; `aar` the binary rect x in [48, 168), y in [24, 200)
/// with its column x = 168 (y in [24, 200)) at 0.6 and its row y = 200 (x in
/// [48, 168)) at 0.3; `ell3` the 8 × 8 supersampled ellipse at (128, 112)
/// with semi-axes 70 × 44; `rr3` the 8 × 8 supersampled rounded rect x in
/// [60, 196), y in [36, 188), corner radius 28.
pub(crate) fn p4_coverage(picture: &str) -> (u32, u32, Vec<f64>) {
    let (w, h) = (256u32, 224u32);
    let a = match picture {
        "disc" | "ell" => return p3_coverage(picture),
        "aar" => (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let rows = (24..200).contains(&y);
                if rows && (48..168).contains(&x) {
                    1.0
                } else if rows && x == 168 {
                    0.6
                } else if y == 200 && (48..168).contains(&x) {
                    0.3
                } else {
                    0.0
                }
            })
            .collect(),
        "ell3" => supersampled(w, h, |x, y| {
            ((x - 128.0) / 70.0).powi(2) + ((y - 112.0) / 44.0).powi(2) <= 1.0
        }),
        "rr3" => supersampled(w, h, |x, y| {
            let (x0, x1, y0, y1, r) = (60.0, 196.0, 36.0, 188.0, 28.0);
            let cx = x.clamp(x0 + r, x1 - r);
            let cy = y.clamp(y0 + r, y1 - r);
            (x0..x1).contains(&x)
                && (y0..y1).contains(&y)
                && (x - cx).powi(2) + (y - cy).powi(2) <= r * r
        }),
        _ => panic!("unknown picture {picture}"),
    };
    (w, h, a)
}

/// The Stroke confirm take's `cfhalf` bands: (coverage, y0, y1) of the x =
/// 256 column (`stroke_aa_578_confirm5.BANDS`).
const TAKE5_BANDS: [(f64, u32, u32); 4] = [
    (0.125, 32, 192),
    (0.375, 192, 352),
    (0.625, 352, 512),
    (0.875, 512, 672),
];

/// The signed distance (negative inside) of a confirm-take curved shape at
/// continuous `(x, y)` (`stroke_aa_578_confirm5.sdf`): the discs `cfdisc17`
/// (r 17.5 at (40, 40)) and `cfdisc30` (r 30 at (48.5, 48.25)), and the
/// exact rounded-box distance of `cfrrect` (centre (64, 48), half extents
/// (44.375, 28.625), corner radius 15.5).
fn take5_sdf(shape: &str, x: f64, y: f64) -> f64 {
    match shape {
        "cfdisc17" => (x - 40.0).hypot(y - 40.0) - 17.5,
        "cfdisc30" => (x - 48.5).hypot(y - 48.25) - 30.0,
        "cfrrect" => {
            let r = 15.5;
            let qx = (x - 64.0).abs() - (44.375 - r);
            let qy = (y - 48.0).abs() - (28.625 - r);
            qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
        }
        _ => panic!("unknown shape {shape}"),
    }
}

/// The Stroke confirm take's picture size and coverage
/// (`stroke_aa_578_confirm5.alpha_of`): `cfhalf` 320 × 704, the rect x in
/// [32, 256), y in [32, 672), its x = 256 column at 0.125 / 0.375 / 0.625 /
/// 0.875 ([`TAKE5_BANDS`]); the curved shapes ([`take5_sdf`]) 8 × 8
/// supersampled, `cfdisc17` 80², `cfdisc30` 96², `cfrrect` 128 × 96.
pub(crate) fn take5_coverage(shape: &str) -> (u32, u32, Vec<f64>) {
    match shape {
        "cfhalf" => {
            let (w, h) = (320u32, 704u32);
            let a = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| {
                    if (32..256).contains(&x) && (32..672).contains(&y) {
                        return 1.0;
                    }
                    if x == 256 {
                        for &(c, y0, y1) in &TAKE5_BANDS {
                            if (y0..y1).contains(&y) {
                                return c;
                            }
                        }
                    }
                    0.0
                })
                .collect();
            (w, h, a)
        }
        _ => {
            let (w, h) = match shape {
                "cfdisc17" => (80u32, 80u32),
                "cfdisc30" => (96, 96),
                "cfrrect" => (128, 96),
                _ => panic!("unknown shape {shape}"),
            };
            (
                w,
                h,
                supersampled(w, h, |x, y| take5_sdf(shape, x, y) <= 0.0),
            )
        }
    }
}

/// A confirm-take curved shape's profile bin of pixel `(x, y)`:
/// `floor(−sdf)` at the pixel centre (`stroke_aa_578_confirm5.bins_of`).
pub(crate) fn take5_bin(shape: &str, x: u32, y: u32) -> i32 {
    (-take5_sdf(shape, f64::from(x) + 0.5, f64::from(y) + 0.5)).floor() as i32
}

/// sRGB's decode of one encoded unit sample (the 32-bit records are linear).
fn linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// `style` applied through the precision edit FFI to layer 1 of `doc`.
fn styled_precision(doc: &PrecisionDocument, style: &Value) -> Box<PrecisionDocument> {
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", common::take_err_string(err));
    unsafe { Box::from_raw(out) }
}

/// The render of a `source`-coloured layer of coverage `alpha` (unit, row
/// major) over an opaque `backdrop` (unit encoded RGB) at `depth`, as codes
/// per channel: 8 bits a `DrDocument` (alpha `floor(255 a + 0.5)`), 16 bits a
/// `PrecisionDocument` of RGBA16 records (`floor(65535 a + 0.5)`, samples /
/// 257), 32 bits a float document (`float_fixture::float_doc`, the 16-bit
/// alpha, linear colours, samples × 255 — linear codes, which equal the
/// encoded ones where every channel is 0 or 1).
pub(crate) fn render_rgb_alpha(
    depth: u32,
    w: u32,
    h: u32,
    alpha: &[f64],
    source: [f64; 3],
    backdrop: [f64; 3],
    style: &Value,
) -> Vec<[f64; 3]> {
    let byte = |v: f64| (v * 255.0 + 0.5).floor() as u8;
    let word = |v: f64| (v * 65535.0 + 0.5).floor() as u16;
    match depth {
        8 => {
            let layer = RgbaImage::from_fn(w, h, |x, y| {
                let a = byte(alpha[(y * w + x) as usize]);
                Rgba([byte(source[0]), byte(source[1]), byte(source[2]), a])
            });
            let base = Rgba([byte(backdrop[0]), byte(backdrop[1]), byte(backdrop[2]), 255]);
            let d = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, base))
                .adding_image_layer(0, layer, "fx")
                .expect("layer");
            common::styled(&d, 1, &style.to_string())
                .flattened()
                .expect("flatten")
                .pixels()
                .map(|p| [0, 1, 2].map(|k| f64::from(p[k])))
                .collect()
        }
        16 => {
            let image = |pixels: Vec<[u16; 4]>| {
                PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).expect("image")
            };
            let s = source.map(word);
            let b = backdrop.map(word);
            let layer = alpha.iter().map(|&a| [s[0], s[1], s[2], word(a)]).collect();
            let doc = PrecisionDocument::from_parts(
                w,
                h,
                IccProfile::srgb(),
                Metadata::default(),
                Resolution::default(),
                vec![
                    Layer::raster(
                        "Base",
                        image(vec![[b[0], b[1], b[2], 65535]; (w * h) as usize]),
                        (0, 0),
                        1.0,
                        true,
                    )
                    .expect("base"),
                    Layer::raster("fx", image(layer), (0, 0), 1.0, true).expect("fx"),
                ],
            )
            .expect("document");
            styled_precision(&doc, style)
                .render(LIMIT, 0, None)
                .expect("render")
                .to_rgba16()
                .expect("rgba16")
                .into_iter()
                .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) / 257.0))
                .collect()
        }
        32 => {
            let s = source.map(|v| linear(v) as f32);
            let b = backdrop.map(|v| linear(v) as f32);
            let layer = alpha
                .iter()
                .map(|&a| [s[0], s[1], s[2], f32::from(word(a)) / 65535.0])
                .collect();
            let doc = crate::float_fixture::float_doc(
                w,
                h,
                &IccProfile::srgb(),
                &[vec![[b[0], b[1], b[2], 1.0]; (w * h) as usize], layer],
            );
            styled_precision(&doc, style)
                .render(LIMIT, 0, None)
                .expect("render")
                .to_rgba32()
                .expect("rgba32")
                .into_iter()
                .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) * 255.0))
                .collect()
        }
        _ => panic!("no render at {depth} bits"),
    }
}

/// The take's profile (`I.profile`) of a render, on the gate.
pub(crate) fn profile(shape: &str, w: u32, codes: &[f64], inside: &[bool]) -> Vec<f64> {
    let at = |x: u32, y: u32| codes[(y * w + x) as usize];
    match shape {
        "half" => (128..=255)
            .map(|x| (128..192).map(|y| at(x, y)).sum::<f64>() / 64.0)
            .collect(),
        "disc" => (0..40)
            .map(|b| {
                let (mut total, mut n) = (0.0, 0usize);
                for y in 0..160 {
                    for x in 0..160 {
                        if inside[(y * w + x) as usize] && disc_distance(x, y).floor() as u32 == b {
                            total += at(x, y);
                            n += 1;
                        }
                    }
                }
                total / n as f64
            })
            .collect(),
        _ => (0..64).map(|k| at(32 + k, 32 + k)).collect(),
    }
}

/// The second take's whole profile (`identify_578_take2.profile`) of a
/// render on the first take's pictures: `half` the column means over rows
/// [128, 192) at x = 128..=319 (192 values); `disc` the mean of every pixel
/// per bin `floor(40 − dist)`, bins -32..=39 (72 values).
pub(crate) fn profile2(shape: &str, w: u32, codes: &[f64]) -> Vec<f64> {
    let at = |x: u32, y: u32| codes[(y * w + x) as usize];
    match shape {
        "half" => (128..=319)
            .map(|x| (128..192).map(|y| at(x, y)).sum::<f64>() / 64.0)
            .collect(),
        "disc" => (-32..=39)
            .map(|b| {
                let (mut total, mut n) = (0.0, 0usize);
                for y in 0..160 {
                    for x in 0..160 {
                        if (40.0 - disc_distance(x, y)).floor() as i32 == b {
                            total += at(x, y);
                            n += 1;
                        }
                    }
                }
                total / n as f64
            })
            .collect(),
        _ => panic!("no take2 profile for {shape}"),
    }
}

/// The third take's profile (`identify_578_take3.profile`, the runner's
/// METRIC) of a render on the first take's pictures: `half` the column means
/// over rows [128, 192) at x = 0..=319 (the take's `left` then its `values`,
/// 320 values); `disc` [`profile2`]'s 72 bins.
pub(crate) fn profile3(shape: &str, w: u32, codes: &[f64]) -> Vec<f64> {
    let at = |x: u32, y: u32| codes[(y * w + x) as usize];
    match shape {
        "half" => (0..=319)
            .map(|x| (128..192).map(|y| at(x, y)).sum::<f64>() / 64.0)
            .collect(),
        "disc" => profile2(shape, w, codes),
        _ => panic!("no take3 profile for {shape}"),
    }
}
