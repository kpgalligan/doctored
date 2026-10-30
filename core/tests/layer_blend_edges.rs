//! The blend-mode EDGE conventions Photoshop 26.10.0 showed at RGB 8, RGB 16
//! and CMYK (`scripts/photoshop-probe/blend_edge_take.py` take1,
//! `doctored-fixtures/photoshop/blend-edge-547/take1`, #547): Divide keeps
//! the backdrop's 0 at 0 / 0 (D-KEEP), and Vivid Light's halves take a black
//! source to 0 and a white one to 1 before the backdrop's ends (VL-PS), while
//! the stand-alone Color Burn and Color Dodge keep the W3C order. Every layer
//! pair of the take's codes is composited through the FFI — an 8-bit
//! `DrDocument` and a 16-bit `PrecisionDocument` — and compared with this
//! file's own exact formulas. Hard Mix's exact tie (`cb + cs = 1`) takes
//! the order rule measured in Photoshop (`scripts/photoshop-probe/gray_luminance_take.py`
//! take1, `doctored-fixtures/photoshop/gray-luminance/take1`, HM-ORD at RGB
//! 8, RGB 16 and CMYK): 1 where `cb − cs > 1/32768`, else 0; its pairs are
//! pinned at both depths. A 16-bit pair Photoshop's 15-bit values tie but the
//! codes do not (32767 / 32767, 32768 / 32768) is not pinned. CMYK's edges are
//! `cmyk_blend.rs`'s (its oracle on `1 − ink` carries the same conventions).

use std::ffi::c_int;

use doctored_core::ffi_doc::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::{Rgba, RgbaImage};
use serde_json::json;
use tempfile::TempDir;

use crate::common::{
    add_layer, apply_or_keep, doc_from, flat_pixels, BLEND_COLOR_BURN, BLEND_COLOR_DODGE,
    BLEND_DIVIDE, BLEND_HARD_MIX, BLEND_VIVID_LIGHT,
};
use crate::native_scalar::edit;

/// The take's 8-bit codes.
const CODES8: [u8; 8] = [0, 1, 64, 127, 128, 191, 254, 255];
/// `gray_luminance_take.py`'s Hard Mix codes: five tie pairs, both orders.
const TIE_CODES8: [u8; 10] = [3, 50, 77, 100, 126, 129, 155, 178, 205, 252];
const MODES: [c_int; 5] = [
    BLEND_DIVIDE,
    BLEND_HARD_MIX,
    BLEND_VIVID_LIGHT,
    BLEND_COLOR_BURN,
    BLEND_COLOR_DODGE,
];

fn burn(cb: f64, cs: f64) -> f64 {
    if cb == 1.0 {
        1.0
    } else if cs == 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - cb) / cs).min(1.0)
    }
}

fn dodge(cb: f64, cs: f64) -> f64 {
    if cb == 0.0 {
        0.0
    } else if cs == 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
}

/// The measured B(cb, cs) for the five modes (module doc). `tie` says
/// whether the codes sum exactly to the top code.
fn measured_blend(mode: c_int, cb: f64, cs: f64, tie: bool) -> f64 {
    match mode {
        BLEND_DIVIDE if cs == 0.0 && cb == 0.0 => 0.0,
        BLEND_DIVIDE if cs == 0.0 => 1.0,
        BLEND_DIVIDE => (cb / cs).min(1.0),
        BLEND_HARD_MIX if tie => {
            if cb - cs > 1.0 / 32768.0 {
                1.0
            } else {
                0.0
            }
        }
        BLEND_HARD_MIX => {
            if cb + cs > 1.0 {
                1.0
            } else {
                0.0
            }
        }
        BLEND_VIVID_LIGHT if cs == 0.0 => 0.0,
        BLEND_VIVID_LIGHT if cs == 1.0 => 1.0,
        BLEND_VIVID_LIGHT if cs <= 0.5 => burn(cb, 2.0 * cs),
        BLEND_VIVID_LIGHT => dodge(cb, 2.0 * cs - 1.0),
        BLEND_COLOR_BURN => burn(cb, cs),
        BLEND_COLOR_DODGE => dodge(cb, cs),
        _ => panic!("mode {mode}"),
    }
}

/// Photoshop's 15-bit value of a 16-bit code.
fn q15(v: u16) -> u32 {
    (u32::from(v) * 32768 + 32767) / 65535
}

/// A 2-wide image: column 0 grey `codes[i]` in row i.
fn column(codes: &[u8], by_row: bool) -> RgbaImage {
    let n = codes.len() as u32;
    RgbaImage::from_fn(n, n, |x, y| {
        let v = codes[(if by_row { y } else { x }) as usize];
        Rgba([v, v, v, 255])
    })
}

fn eight_bit_layers(codes: &[u8], modes: &[c_int]) {
    let dir = TempDir::new().unwrap();
    let n = codes.len() as u32;
    for &mode in modes {
        let doc = doc_from(&dir, &format!("b{mode}.png"), &column(codes, true));
        let doc = add_layer(
            &dir,
            &format!("s{mode}.png"),
            doc,
            0,
            &column(codes, false),
            "Top",
        );
        let doc = apply_or_keep(doc, |d| unsafe { dr_doc_with_layer_blend_mode(d, 1, mode) });
        let flat = flat_pixels(doc);
        unsafe { dr_doc_free(doc) };
        for (r, &b) in codes.iter().enumerate() {
            for (c, &s) in codes.iter().enumerate() {
                let tie = u32::from(b) + u32::from(s) == 255;
                let want = measured_blend(mode, f64::from(b) / 255.0, f64::from(s) / 255.0, tie);
                let want = (want * 255.0).round();
                let at = ((r as u32 * n + c as u32) * 4) as usize;
                for ch in 0..3 {
                    let got = f64::from(flat[at + ch]);
                    assert!(
                        (got - want).abs() <= 1.0,
                        "mode {mode} backdrop {b} source {s}: {got} vs {want}"
                    );
                }
            }
        }
    }
}

#[test]
fn eight_bit_layers_take_the_measured_edges() {
    eight_bit_layers(&CODES8, &MODES);
}

#[test]
fn eight_bit_hard_mix_ties_take_the_measured_order() {
    eight_bit_layers(&TIE_CODES8, &[BLEND_HARD_MIX]);
}

/// The blend-edge take's 16-bit codes: the eight × 257, then 32767 and 32768.
fn codes16() -> Vec<u16> {
    let mut out: Vec<u16> = CODES8.iter().map(|&v| u16::from(v) * 257).collect();
    out.extend([32767, 32768]);
    out
}

/// The Hard Mix take's 16-bit codes: the ten × 257, then 32766 to 32769.
fn tie_codes16() -> Vec<u16> {
    let mut out: Vec<u16> = TIE_CODES8.iter().map(|&v| u16::from(v) * 257).collect();
    out.extend([32766, 32767, 32768, 32769]);
    out
}

fn grid16(codes: &[u16], by_row: bool) -> PrecisionImage {
    let n = codes.len() as u32;
    let pixels = (0..n * n)
        .map(|i| {
            let v = codes[(if by_row { i / n } else { i % n }) as usize];
            [v, v, v, 65535]
        })
        .collect();
    PrecisionImage::from_rgba16(n, n, pixels, IccProfile::srgb(), None).unwrap()
}

fn sixteen_bit_layers(codes: &[u16], modes: &[c_int]) {
    let n = codes.len() as u32;
    let srgb = IccProfile::srgb();
    let layers = vec![
        PrecisionDocumentLayer::raster("Back", grid16(codes, true), (0, 0), 1.0, true).unwrap(),
        PrecisionDocumentLayer::raster("Top", grid16(codes, false), (0, 0), 1.0, true).unwrap(),
    ];
    let base = PrecisionDocument::from_parts(
        n,
        n,
        srgb,
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    for &mode in modes {
        let doc = edit(
            &base,
            json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":mode}),
            &[],
        )
        .expect("the mode changes the layer");
        let rendered = doc.render(1 << 31, 0, None).unwrap().to_rgba16().unwrap();
        for (r, &b) in codes.iter().enumerate() {
            for (c, &s) in codes.iter().enumerate() {
                let tie = u32::from(b) + u32::from(s) == 65535;
                if mode == BLEND_HARD_MIX && (q15(b) + q15(s) == 32768) != tie {
                    continue;
                }
                let want =
                    measured_blend(mode, f64::from(b) / 65535.0, f64::from(s) / 65535.0, tie);
                let want = (want * 65535.0).round();
                let got = rendered[r * n as usize + c];
                for &channel in &got[..3] {
                    assert!(
                        (f64::from(channel) - want).abs() <= 3.0,
                        "mode {mode} backdrop {b} source {s}: {channel} vs {want}"
                    );
                }
            }
        }
    }
}

#[test]
fn sixteen_bit_layers_take_the_measured_edges() {
    sixteen_bit_layers(&codes16(), &MODES);
}

#[test]
fn sixteen_bit_hard_mix_ties_take_the_measured_order() {
    sixteen_bit_layers(&tie_codes16(), &[BLEND_HARD_MIX]);
}
