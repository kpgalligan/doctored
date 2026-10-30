//! Dodge/Burn's Protect Tones (#547) at 8, 16 and 32 bits, through the FFI,
//! against an f64 statement of the rule written here: the curve
//! (`shaped`, the tool's documented black-point, gamma and white-point
//! moves) and the group rule — with `V` the pixel's largest channel, every
//! channel × `shaped(V) / V` (`V = 0`: `shaped(0)` each) — then the
//! coverage lerp. Off, each channel takes the plain curve. In a float
//! document the group is the display encoding E of the in-range channels
//! (the sRGB curve, written here) and a channel above white keeps the plain
//! white-point scale bit for bit.

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::{dr_doc_dodge_burn_layer, dr_doc_free};
use doctored_core::icc::IccProfile;
use image::{Rgba, RgbaImage};
use serde_json::json;

use crate::float_fixture::float_doc;
use crate::native_scalar as native;

fn shaped(v: f64, e: f64, range: u8, burn: bool) -> f64 {
    let k = e / 3.0;
    match (range, burn) {
        (0, false) => k + v * (1.0 - k),
        (0, true) => ((v - k) / (1.0 - k)).max(0.0),
        (1, false) => v.powf(1.0 / (1.0 + e)),
        (1, true) => v.powf(1.0 + e),
        (2, false) => (v / (1.0 - k)).min(1.0),
        _ => v * (1.0 - k),
    }
}

/// The targets of one pixel's three channels.
fn targets(rgb: [f64; 3], e: f64, range: u8, burn: bool, protect: bool) -> [f64; 3] {
    if !protect {
        return rgb.map(|v| shaped(v, e, range, burn));
    }
    let v = rgb[0].max(rgb[1]).max(rgb[2]);
    if v <= 0.0 {
        return [shaped(0.0, e, range, burn); 3];
    }
    let scale = shaped(v, e, range, burn) / v;
    rgb.map(|c| c * scale)
}

const CASES: [(u8, bool); 4] = [(0, false), (1, true), (2, false), (2, true)];

#[test]
fn eight_bit_protect_tones_matches_the_group_rule() {
    let img = RgbaImage::from_fn(16, 16, |x, y| {
        let i = y * 16 + x;
        Rgba([
            i as u8,
            (255 - i) as u8 / 2,
            ((i * 7 + 13) % 256) as u8,
            255,
        ])
    });
    let doc = DrDocument::from_pixels(img.clone());
    let overlay: Vec<u8> = (0..256u32)
        .flat_map(|i| [0, 0, 0, (i * 3 % 256) as u8])
        .collect();
    for (range, burn) in CASES {
        for protect in [false, true] {
            let handle = Box::into_raw(Box::new(doc.clone()));
            let out = unsafe {
                dr_doc_dodge_burn_layer(
                    handle,
                    0,
                    overlay.as_ptr(),
                    16,
                    16,
                    0.7,
                    range,
                    burn,
                    protect,
                )
            };
            unsafe { dr_doc_free(handle) };
            assert!(!out.is_null());
            let out = unsafe { Box::from_raw(out) };
            let after = out.layers[0].pixels.contiguous_image();
            for (i, (b, a)) in img.pixels().zip(after.pixels()).enumerate() {
                let c = f64::from(overlay[i * 4 + 3]) / 255.0;
                let t = targets(
                    b.0.map(|v| f64::from(v) / 255.0)[..3].try_into().unwrap(),
                    0.7,
                    range,
                    burn,
                    protect,
                );
                for (ch, &target) in t.iter().enumerate() {
                    let v = f64::from(b.0[ch]) / 255.0;
                    let want = ((v + c * (target - v)) * 255.0 + 0.5).floor();
                    assert!(
                        (f64::from(a.0[ch]) - want).abs() <= 1.0,
                        "range {range} burn {burn} protect {protect} pixel {i} ch {ch}: {} vs {want}",
                        a.0[ch]
                    );
                }
                assert_eq!(a.0[3], b.0[3]);
            }
        }
    }
}

#[test]
fn sixteen_bit_protect_tones_matches_the_group_rule() {
    let (w, h) = (12u32, 9u32);
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let j = i.wrapping_mul(2_654_435_761);
            [
                (j % 65536) as u16,
                (j.rotate_left(9) % 65536) as u16,
                (j.rotate_left(19) % 30000) as u16,
                if i == 5 { 0 } else { 65535 },
            ]
        })
        .collect();
    let doc = native::doc(w, h, pixels.clone());
    let coverage: Vec<u16> = (0..w * h).map(|i| (i * 977 % 65536) as u16).collect();
    for (range, burn) in CASES {
        for protect in [false, true] {
            let out = native::edit(
                &doc,
                json!({"op":"dodge_burn","layer":0,"exposure":0.5,"range":range,"burn":burn,"protect_tones":protect}),
                &coverage,
            )
            .unwrap();
            let after = out.layer(0).unwrap().source().unwrap().to_rgba16().unwrap();
            for (i, (b, a)) in pixels.iter().zip(&after).enumerate() {
                if b[3] == 0 {
                    assert_eq!(a, b, "a transparent pixel is skipped");
                    continue;
                }
                let c = f64::from(coverage[i]) / 65535.0;
                let rgb = [0, 1, 2].map(|ch| f64::from(b[ch]) / 65535.0);
                let t = targets(rgb, 0.5, range, burn, protect);
                for ch in 0..3 {
                    let want = (f64::from(b[ch]) * (1.0 - c) + t[ch] * 65535.0 * c).round();
                    assert!(
                        (f64::from(a[ch]) - want).abs() <= 1.0,
                        "range {range} burn {burn} protect {protect} pixel {i} ch {ch}: {} vs {want}",
                        a[ch]
                    );
                }
            }
        }
    }
}

fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

#[test]
fn float_protect_tones_moves_the_encoded_triple_as_one_group() {
    let pixels: Vec<[f32; 4]> = vec![
        [0.6, 0.2, 0.05, 1.0],
        [0.3, 0.3, 0.3, 1.0],
        [0.02, 0.4, 0.15, 1.0],
        [2.0, 0.5, 0.1, 1.0],
        [0.0, 0.0, 0.0, 1.0],
        [0.8, 0.7, 0.75, 0.5],
    ];
    let (w, h) = (pixels.len() as u32, 1u32);
    let doc = float_doc(w, h, &IccProfile::srgb(), std::slice::from_ref(&pixels));
    let full = vec![65535u16; pixels.len()];
    for (range, burn) in CASES {
        let run = |protect: bool| -> Vec<[f32; 4]> {
            let out = native::edit(
                &doc,
                json!({"op":"dodge_burn","layer":0,"exposure":0.6,"range":range,"burn":burn,"protect_tones":protect}),
                &full,
            )
            .unwrap();
            let image = out.layer(0).unwrap().source().unwrap();
            (0..w).map(|x| image.sample_at(x, 0).unwrap()).collect()
        };
        let (plain, protected) = (run(false), run(true));
        for (i, p) in pixels.iter().enumerate() {
            let rgb = [0, 1, 2].map(|c| f64::from(p[c]));
            let encoded = rgb.map(encode);
            let want = targets(encoded, 0.6, range, burn, true);
            for c in 0..3 {
                if p[c] > 1.0 {
                    assert_eq!(
                        protected[i][c].to_bits(),
                        plain[i][c].to_bits(),
                        "pixel {i}: above white keeps the white-point scale"
                    );
                    continue;
                }
                let got = encode(f64::from(protected[i][c]));
                assert!(
                    (got - want[c]).abs() <= 2e-3,
                    "range {range} burn {burn} pixel {i} ch {c}: E {got} vs {}",
                    want[c]
                );
            }
            assert_eq!(
                protected[i][3].to_bits(),
                p[3].to_bits(),
                "alpha is never written"
            );
            if p[0] == p[1] && p[1] == p[2] {
                for c in 0..3 {
                    assert!(
                        (protected[i][c] - plain[i][c]).abs() <= 1e-6,
                        "a neutral is the plain curve"
                    );
                }
            }
        }
    }
}
