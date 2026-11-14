//! Frequency separation's arithmetic, through the FFI: the detail layer is
//! Apply Image's Subtract of the STORED low layer at scale 2 and offset ½
//! (`dr_blend_planes_rgb` at 8 bits, the typed `"kind":"blend"` at 16), and a
//! Linear Light layer clipped to the low layer recomposes the original within
//! one code. The low is a real Gaussian and, separately, an unrelated noise
//! image: the bound holds for ANY low, because the detail is computed from
//! the codes the low layer stores.
//!
//! Why ½ and not 128 levels: with `d = src − low` in codes, the detail stores
//! `(d + max) / 2`, and `max` is odd at both depths, so an odd `d` is exact
//! and an even one is half a code off, which Linear Light (`low + 2·high −
//! max`) doubles to at most one code. A pivot of 128 (or 32768) adds half a
//! code to every sample, and the two halves can then reach two codes; each
//! test shows that control failing on the same images, so the images are not
//! too tame to tell the pivots apart.
use std::ffi::CString;
use std::ptr;

use crate::common::{BLEND_LINEAR_LIGHT, BLEND_SUBTRACT};
use crate::native_scalar::{error, text, LIMIT};
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_channel::dr_blend_planes_rgb;
use doctored_core::ffi_doc::{
    dr_doc_adding_image_layer, dr_doc_flattened, dr_doc_free, dr_doc_from_image,
    dr_doc_with_layer_blend_mode, dr_doc_with_layer_clipped,
};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_selection_u16::dr_selection_u16;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;
use serde_json::json;

const W: u32 = 128;
const H: u32 = 96;

/// A deterministic xorshift.
struct Noise(u64);
impl Noise {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Opaque noise over hard edges: strong detail at every scale.
fn picture(max: u32, seed: u64) -> Vec<[u32; 4]> {
    let mut noise = Noise(seed);
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let edge = if (x / 17 + y / 11) % 2 == 0 {
                max / 5
            } else {
                max - max / 6
            };
            let rgb: [u32; 3] = std::array::from_fn(|c| {
                let jitter = (noise.next() % u64::from(max / 3)) as u32;
                (edge + jitter + c as u32 * max / 9).min(max)
            });
            [rgb[0], rgb[1], rgb[2], max]
        })
        .collect()
}

/// An unrelated opaque noise image: a low no blur would give.
fn unrelated(max: u32, seed: u64) -> Vec<[u32; 4]> {
    let mut noise = Noise(seed);
    (0..W * H)
        .map(|_| {
            let rgb: [u32; 3] = std::array::from_fn(|_| (noise.next() % u64::from(max + 1)) as u32);
            [rgb[0], rgb[1], rgb[2], max]
        })
        .collect()
}

fn image(pixels: &[[u32; 4]]) -> *mut DrImage {
    let bytes: Vec<u8> = pixels.iter().flat_map(|p| p.map(|v| v as u8)).collect();
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), W, H) };
    assert!(!img.is_null());
    img
}

fn rgba(img: *const DrImage) -> Vec<[u32; 4]> {
    let data =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(img), (W * H * 4) as usize) };
    data.chunks_exact(4)
        .map(|p| [p[0].into(), p[1].into(), p[2].into(), p[3].into()])
        .collect()
}

/// The byte Gaussian Blur the Filter menu runs at 8 bits.
fn byte_blur(src: &[[u32; 4]], sigma: f64) -> Vec<[u32; 4]> {
    let img = image(src);
    let op = CString::new("blur").unwrap();
    let params = text(json!({ "sigma": sigma }));
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", unsafe { error(err) });
    let pixels = rgba(out);
    unsafe {
        dr_image_free(img);
        dr_image_free(out);
    }
    pixels
}

/// Apply Image's Subtract at scale 2 and `offset` (plane units) of `low`
/// from `src`, at 8 bits; the detail keeps the source's alpha.
fn byte_detail(src: &[[u32; 4]], low: &[[u32; 4]], offset: f32) -> Vec<[u32; 4]> {
    let plane = |p: &[[u32; 4]], c: usize| -> Vec<u8> { p.iter().map(|v| v[c] as u8).collect() };
    let mut base = [plane(src, 0), plane(src, 1), plane(src, 2)];
    let source = [plane(low, 0), plane(low, 1), plane(low, 2)];
    let [r, g, b] = &mut base;
    let ok = unsafe {
        dr_blend_planes_rgb(
            r.as_mut_ptr(),
            g.as_mut_ptr(),
            b.as_mut_ptr(),
            source[0].as_ptr(),
            source[1].as_ptr(),
            source[2].as_ptr(),
            W,
            H,
            BLEND_SUBTRACT,
            1.0,
            false,
            false,
            2.0,
            offset,
        )
    };
    assert!(ok);
    (0..src.len())
        .map(|i| {
            [
                base[0][i].into(),
                base[1][i].into(),
                base[2][i].into(),
                src[i][3],
            ]
        })
        .collect()
}

/// Low below, detail above it in Linear Light and clipped to it, flattened.
fn byte_recomposed(low: &[[u32; 4]], high: &[[u32; 4]]) -> Vec<[u32; 4]> {
    let (low_img, high_img) = (image(low), image(high));
    let name = CString::new("High").unwrap();
    unsafe {
        let doc = dr_doc_from_image(low_img);
        let two = dr_doc_adding_image_layer(doc, 0, high_img, name.as_ptr());
        let blended = dr_doc_with_layer_blend_mode(two, 1, BLEND_LINEAR_LIGHT);
        let clipped = dr_doc_with_layer_clipped(blended, 1, true);
        assert!(!clipped.is_null());
        let mut err = ptr::null_mut();
        let flat = dr_doc_flattened(clipped, 0, ptr::null(), &mut err);
        assert!(!flat.is_null(), "{}", error(err));
        let out = rgba(flat);
        dr_image_free(flat);
        for d in [doc, two, blended, clipped] {
            dr_doc_free(d);
        }
        dr_image_free(low_img);
        dr_image_free(high_img);
        out
    }
}

fn max_error(a: &[[u32; 4]], b: &[[u32; 4]]) -> u32 {
    a.iter()
        .zip(b)
        .flat_map(|(p, q)| (0..3).map(move |c| p[c].abs_diff(q[c])))
        .max()
        .unwrap_or(0)
}

#[test]
fn eight_bit_split_recomposes_within_one_code() {
    let src = picture(255, 0x9e37_79b9_7f4a_7c15);
    let lows: Vec<(String, Vec<[u32; 4]>)> = [0.7, 2.0, 4.0, 12.0]
        .iter()
        .map(|&sigma| (format!("blur {sigma}"), byte_blur(&src, sigma)))
        .chain(std::iter::once((
            "unrelated".to_string(),
            unrelated(255, 0x1234_5678),
        )))
        .collect();
    for (label, low) in &lows {
        let high = byte_detail(&src, low, 0.5);
        // The detail is (d + 255) / 2 of the stored codes: exact for an odd
        // d, either neighbour of the half for an even one.
        for i in 0..src.len() {
            for c in 0..3 {
                let twice = src[i][c] as i64 - low[i][c] as i64 + 255;
                let got = high[i][c] as i64;
                assert!(
                    got * 2 == twice || (got * 2 - twice).abs() == 1,
                    "{label}: pixel {i} channel {c}: detail {got} for d + 255 = {twice}"
                );
            }
        }
        let error = max_error(&byte_recomposed(low, &high), &src);
        assert!(error <= 1, "{label}: recomposed within {error} codes");
        // The control: a pivot of 128 levels is two codes off somewhere.
        let control = byte_detail(&src, low, 128.0 / 255.0);
        assert_eq!(
            max_error(&byte_recomposed(low, &control), &src),
            2,
            "{label}: pivot 128"
        );
    }
}

fn typed_image(pixels: &[[u32; 4]]) -> PrecisionImage {
    let samples = pixels.iter().map(|p| p.map(|v| v as u16)).collect();
    PrecisionImage::from_rgba16(W, H, samples, IccProfile::srgb(), None).unwrap()
}

/// A 3×3 box mean, written here: a smooth 16-bit low.
fn box_low(src: &[[u32; 4]]) -> Vec<[u32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = ((i % W) as i64, (i / W) as i64);
            let mut sum = [0u64; 3];
            let mut n = 0u64;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= W as i64 || sy >= H as i64 {
                        continue;
                    }
                    let p = src[(sy * W as i64 + sx) as usize];
                    for c in 0..3 {
                        sum[c] += u64::from(p[c]);
                    }
                    n += 1;
                }
            }
            let rgb: [u32; 3] = std::array::from_fn(|c| ((sum[c] + n / 2) / n) as u32);
            [rgb[0], rgb[1], rgb[2], 65535]
        })
        .collect()
}

/// The typed plane op's Subtract at scale 2 and `offset` of `low` from `src`.
fn typed_detail(src: &[[u32; 4]], low: &[[u32; 4]], offset: f64) -> Vec<[u32; 4]> {
    let n = src.len();
    let mut input: Vec<u16> = Vec::with_capacity(6 * n);
    for image in [src, low] {
        for c in 0..3 {
            input.extend(image.iter().map(|p| p[c] as u16));
        }
    }
    let request = text(
        json!({"kind":"blend","channels":3,"mode":BLEND_SUBTRACT,"opacity":1,
        "scale":2,"offset":offset}),
    );
    let mut out = vec![0u16; 3 * n];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_selection_u16(
            W,
            H,
            request.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", unsafe { error(err) });
    (0..n)
        .map(|i| {
            [
                out[i].into(),
                out[n + i].into(),
                out[2 * n + i].into(),
                src[i][3],
            ]
        })
        .collect()
}

fn typed_recomposed(low: &[[u32; 4]], high: &[[u32; 4]]) -> Vec<[u32; 4]> {
    let layers = vec![
        PrecisionDocumentLayer::raster("Low", typed_image(low), (0, 0), 1.0, true).unwrap(),
        PrecisionDocumentLayer::raster("High", typed_image(high), (0, 0), 1.0, true)
            .unwrap()
            .with_clipped(true),
    ];
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        layers,
    )
    .unwrap();
    let request = CString::new(format!(
        r#"{{"op":"layer_blend","version":1,"layer":1,"blend_mode":{BLEND_LINEAR_LIGHT}}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let blended = unsafe {
        dr_precision_doc_edit(
            &doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!blended.is_null(), "{}", unsafe { error(err) });
    let blended = unsafe { Box::from_raw(blended) };
    let render = blended.render(LIMIT, 0, None).unwrap();
    (0..W * H)
        .map(|i| render.rgba16_at(i % W, i / W).unwrap().map(u32::from))
        .collect()
}

#[test]
fn sixteen_bit_split_recomposes_within_one_code() {
    let src = picture(65535, 0x2545_f491_4f6c_dd1d);
    for (label, low) in [
        ("box", box_low(&src)),
        ("unrelated", unrelated(65535, 0xdead_beef)),
    ] {
        let high = typed_detail(&src, &low, 0.5);
        for i in 0..src.len() {
            for c in 0..3 {
                let twice = src[i][c] as i64 - low[i][c] as i64 + 65535;
                let got = high[i][c] as i64;
                assert!(
                    got * 2 == twice || (got * 2 - twice).abs() == 1,
                    "{label}: pixel {i} channel {c}: detail {got} for d + 65535 = {twice}"
                );
            }
        }
        let error = max_error(&typed_recomposed(&low, &high), &src);
        assert!(error <= 1, "{label}: recomposed within {error} codes");
        let control = typed_detail(&src, &low, 32768.0 / 65535.0);
        assert_eq!(
            max_error(&typed_recomposed(&low, &control), &src),
            2,
            "{label}: pivot 32768"
        );
    }
}
