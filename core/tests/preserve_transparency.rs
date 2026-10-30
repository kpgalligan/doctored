//! Preserve Transparency / Lock Transparency on paint, at
//! 8, 16 and 32 bits, black-box through the FFI and the typed request API.
//!
//! The oracle is independent f64 arithmetic: a pixel at alpha 0 is untouched,
//! any other pixel takes `Cb + sa·(B(Cb, Cs) − Cb)` in its colour and keeps
//! its alpha exactly (at alpha 1 that IS the W3C step, so opaque pixels are
//! unchanged from the free path). Byte and u16 results agree within one
//! code; f32 within a relative 1e-6.

use std::ffi::c_int;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_fill_stroke::{
    dr_doc_fill_selection, dr_doc_stroke_layer_edge, dr_selection_stroke,
};
use doctored_core::ffi_group::dr_doc_with_layer_locks;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::tiff_float::FloatTiff;
use image::{Rgba, RgbaImage};
use serde_json::json;

use crate::common;
use crate::native_scalar as native;
use common::*;

const INK: [u8; 3] = [200, 100, 50];
const W: u32 = 24;
const H: u32 = 18;
const INSIDE: c_int = 0;
const CENTER: c_int = 1;

// --------------------------------------------------------------- oracle --

/// The independent f64 separable `B(cb, cs)` for the modes tested here.
fn blend(mode: c_int, cb: f64, cs: f64) -> f64 {
    match mode {
        BLEND_NORMAL => cs,
        BLEND_MULTIPLY => cb * cs,
        BLEND_SCREEN => cb + cs - cb * cs,
        BLEND_DARKEN => cb.min(cs),
        _ => unreachable!("not a tested mode"),
    }
}

/// The frozen-alpha rule on one byte pixel.
fn frozen8(dp: [u8; 4], cs: [f64; 3], sa: f64, mode: c_int) -> [u8; 4] {
    if dp[3] == 0 || sa <= 0.0 {
        return dp;
    }
    let mut out = dp;
    for c in 0..3 {
        let cb = f64::from(dp[c]) / 255.0;
        let v = cb + sa * (blend(mode, cb, cs[c]) - cb);
        out[c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out
}

/// The frozen-alpha rule on one u16 pixel.
fn frozen16(dp: [u16; 4], cs: [f64; 3], sa: f64, mode: c_int) -> [u16; 4] {
    if dp[3] == 0 || sa <= 0.0 {
        return dp;
    }
    let mut out = dp;
    for c in 0..3 {
        let cb = f64::from(dp[c]) / 65535.0;
        let v = cb + sa * (blend(mode, cb, cs[c]) - cb);
        out[c] = (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    }
    out
}

fn near8(got: [u8; 4], want: [u8; 4], what: &str) {
    assert_eq!(got[3], want[3], "{what}: alpha is kept — got {got:?}");
    for c in 0..3 {
        assert!(
            (i32::from(got[c]) - i32::from(want[c])).abs() <= 1,
            "{what}: got {got:?}, want {want:?}"
        );
    }
}

fn near16(got: [u16; 4], want: [u16; 4], what: &str) {
    assert_eq!(got[3], want[3], "{what}: alpha is kept — got {got:?}");
    for c in 0..3 {
        assert!(
            (i32::from(got[c]) - i32::from(want[c])).abs() <= 1,
            "{what}: got {got:?}, want {want:?}"
        );
    }
}

// ------------------------------------------------------------- fixtures --

/// Alphas 0, a spread of semi-transparent values and 255.
fn semi(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        let a = match i % 7 {
            0 => 0,
            1 => 255,
            k => (k * 37 + i % 11) as u8,
        };
        Rgba([
            (i * 37 % 256) as u8,
            (i * 11 % 256) as u8,
            (90 + i * 7) as u8,
            a,
        ])
    })
}

fn mask(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .map(|i| [0u8, 255, 128, 40, 3, 200][i as usize % 6])
        .collect()
}

fn px(buf: &[u8], i: usize) -> [u8; 4] {
    [buf[4 * i], buf[4 * i + 1], buf[4 * i + 2], buf[4 * i + 3]]
}

fn take(out: *mut DrDocument) -> Vec<u8> {
    assert!(!out.is_null(), "the edit changes the layer");
    let pixels = layer_pixels(out, 0);
    unsafe { dr_doc_free(out) };
    pixels
}

fn transparency_locked(doc: &DrDocument) -> DrDocument {
    let out = unsafe { dr_doc_with_layer_locks(doc, 0, LOCK_TRANSPARENCY) };
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

fn fill(
    doc: &DrDocument,
    mask: &[u8],
    rgba: [u8; 4],
    mode: c_int,
    opacity: f32,
    preserve: bool,
) -> *mut DrDocument {
    let (w, h) = (doc.width, doc.height);
    unsafe {
        dr_doc_fill_selection(
            doc,
            0,
            mask.as_ptr(),
            w,
            h,
            rgba.as_ptr(),
            mode,
            opacity,
            preserve,
        )
    }
}

/// The byte fill oracle over `mask` (the fill's coverage alpha
/// `(255·cov + 127) / 255` is `cov` at full colour alpha).
fn fill_oracle(base: &[u8], mask: &[u8], mode: c_int, opacity: f32) -> Vec<[u8; 4]> {
    let cs = INK.map(|v| f64::from(v) / 255.0);
    (0..mask.len())
        .map(|i| {
            frozen8(
                px(base, i),
                cs,
                f64::from(mask[i]) / 255.0 * f64::from(opacity),
                mode,
            )
        })
        .collect()
}

// ----------------------------------------------------------------- byte --

#[test]
fn byte_fill_flag_and_lock_follow_the_lerp() {
    let base = semi(W, H);
    let doc = DrDocument::from_pixels(base.clone());
    let locked = transparency_locked(&doc);
    let m = mask(W, H);
    let rgba = [INK[0], INK[1], INK[2], 255];
    for mode in [BLEND_NORMAL, BLEND_MULTIPLY, BLEND_SCREEN] {
        let want = fill_oracle(base.as_raw(), &m, mode, 0.7);
        let flag = take(fill(&doc, &m, rgba, mode, 0.7, true));
        let lock = take(fill(&locked, &m, rgba, mode, 0.7, false));
        for (i, w) in want.iter().enumerate() {
            near8(px(&flag, i), *w, &format!("flag mode {mode} pixel {i}"));
            near8(px(&lock, i), *w, &format!("lock mode {mode} pixel {i}"));
        }
    }
}

#[test]
fn byte_selection_stroke_follows_the_lerp() {
    let base = semi(W, H);
    let doc = DrDocument::from_pixels(base.clone());
    let locked = transparency_locked(&doc);
    let selection: Vec<u8> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (5..19).contains(&x) && (4..14).contains(&y) {
                255
            } else {
                0
            }
        })
        .collect();
    let mut band = selection.clone();
    assert!(unsafe { dr_selection_stroke(band.as_mut_ptr(), W, H, 2.5, CENTER) });
    let rgba = [INK[0], INK[1], INK[2], 255];
    for mode in [BLEND_NORMAL, BLEND_MULTIPLY, BLEND_SCREEN] {
        let want = fill_oracle(base.as_raw(), &band, mode, 0.9);
        let flag = take(fill(&doc, &band, rgba, mode, 0.9, true));
        let lock = take(fill(&locked, &band, rgba, mode, 0.9, false));
        for (i, w) in want.iter().enumerate() {
            near8(px(&flag, i), *w, &format!("flag mode {mode} pixel {i}"));
            near8(px(&lock, i), *w, &format!("lock mode {mode} pixel {i}"));
        }
    }
}

/// A 40×40 layer holding a semi-transparent disc well inside the canvas:
/// alpha 0 outside, a varied alpha inside.
fn disc() -> RgbaImage {
    RgbaImage::from_fn(40, 40, |x, y| {
        let d = ((x as f32 + 0.5 - 20.0).powi(2) + (y as f32 + 0.5 - 20.0).powi(2)).sqrt();
        let a = if d < 11.0 {
            (60 + (x * 7 + y * 3) % 190) as u8
        } else {
            0
        };
        Rgba([(x * 6) as u8, (y * 6) as u8, 140, a])
    })
}

#[test]
fn byte_layer_edge_stroke_follows_the_lerp() {
    let base = disc();
    let doc = DrDocument::from_pixels(base.clone());
    let locked = transparency_locked(&doc);
    // The band the layer-edge stroke fills: its own alpha plane through the
    // selection stroke (the canvas edge is inert this far from the disc).
    let mut band: Vec<u8> = base.pixels().map(|p| p[3]).collect();
    assert!(unsafe { dr_selection_stroke(band.as_mut_ptr(), 40, 40, 3.0, INSIDE) });
    let rgba = [INK[0], INK[1], INK[2], 255];
    for mode in [BLEND_NORMAL, BLEND_MULTIPLY, BLEND_SCREEN] {
        let want = fill_oracle(base.as_raw(), &band, mode, 0.8);
        for (d, preserve) in [(&doc, true), (&locked, false)] {
            let mut err = ptr::null_mut();
            let out = unsafe {
                dr_doc_stroke_layer_edge(
                    d,
                    0,
                    3.0,
                    INSIDE,
                    rgba.as_ptr(),
                    mode,
                    0.8,
                    preserve,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(err.is_null(), "{}", take_err_string(err));
            let got = take(out);
            for (i, w) in want.iter().enumerate() {
                near8(
                    px(&got, i),
                    *w,
                    &format!("preserve {preserve} mode {mode} pixel {i}"),
                );
            }
        }
    }
}

/// `premul(INK, a)` per pixel over a spread of overlay alphas.
fn brush_overlay(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let a = [0u8, 3, 40, 128, 200, 255][i as usize % 6];
            let c = |v: u8| ((u32::from(v) * u32::from(a) + 127) / 255) as u8;
            [c(INK[0]), c(INK[1]), c(INK[2]), a]
        })
        .collect()
}

#[test]
fn byte_brush_under_the_lock_follows_the_lerp() {
    let base = semi(W, H);
    let locked = transparency_locked(&DrDocument::from_pixels(base.clone()));
    let src = brush_overlay(W, H);
    let over =
        take(unsafe { dr_doc_painting_layer(&locked, 0, src.as_ptr(), W, H, COMPOSITE_OVER, 0.8) });
    let blended = take(unsafe {
        dr_doc_painting_layer_blend(&locked, 0, src.as_ptr(), W, H, BLEND_MULTIPLY, 0.8)
    });
    let coloured = take(unsafe {
        dr_doc_painting_layer_blend_color(
            &locked,
            0,
            src.as_ptr(),
            W,
            H,
            INK.as_ptr(),
            BLEND_MULTIPLY,
            0.8,
        )
    });
    for i in 0..(W * H) as usize {
        let sp = px(&src, i);
        let dp = px(base.as_raw(), i);
        let sa = f64::from(sp[3]) / 255.0 * 0.8;
        // The overlay entries read the overlay's own colour, un-premultiplied.
        let overlay_cs = if sp[3] == 0 {
            [0.0; 3]
        } else {
            [0, 1, 2].map(|c| (f64::from(sp[c]) / f64::from(sp[3])).min(1.0))
        };
        let exact = INK.map(|v| f64::from(v) / 255.0);
        near8(
            px(&over, i),
            frozen8(dp, overlay_cs, sa, BLEND_NORMAL),
            &format!("over {i}"),
        );
        near8(
            px(&blended, i),
            frozen8(dp, overlay_cs, sa, BLEND_MULTIPLY),
            &format!("blend {i}"),
        );
        near8(
            px(&coloured, i),
            frozen8(dp, exact, sa, BLEND_MULTIPLY),
            &format!("colour {i}"),
        );
    }
}

#[test]
fn byte_bucket_and_gradient_under_the_lock_follow_the_lerp() {
    let base = semi(W, H);
    let locked = transparency_locked(&DrDocument::from_pixels(base.clone()));
    let paint = [INK[0], INK[1], INK[2], 153];
    let bucket = take(unsafe {
        dr_doc_bucket_fill(&locked, 0, 3, 3, 255, paint.as_ptr(), true, ptr::null())
    });
    let gradient = take(unsafe {
        dr_doc_gradient(
            &locked,
            0,
            0.0,
            0.0,
            W as f32,
            H as f32,
            paint.as_ptr(),
            paint.as_ptr(),
            0,
            ptr::null(),
        )
    });
    let cs = INK.map(|v| f64::from(v) / 255.0);
    // The bucket admits by the seven projections (R, G, B, A, R−G,
    // G−B, R−B within ±tolerance of the seed's), so tolerance 255 no longer
    // floods every pixel of this saturated picture: the region is this flood,
    // and outside it the bucket leaves the layer as it was.
    let filled = seven_projection_flood(&base, (3, 3), 255);
    for (i, &inside) in filled.iter().enumerate() {
        let want = frozen8(px(base.as_raw(), i), cs, 0.6, BLEND_NORMAL);
        let kept = px(base.as_raw(), i);
        near8(
            px(&bucket, i),
            if inside { want } else { kept },
            &format!("bucket {i}"),
        );
        near8(px(&gradient, i), want, &format!("gradient {i}"));
    }
    // The flood reaches past the seed into semi-transparent pixels, and stops.
    let reached = (0..(W * H) as usize)
        .filter(|&i| filled[i] && (1..255).contains(&px(base.as_raw(), i)[3]))
        .count();
    assert!(reached > 0 && filled.iter().any(|&f| !f), "{reached}");
}

/// The bucket's region, written independently: four-connected from `seed`
/// over the pixels whose R, G, B, A, R−G, G−B and R−B each lie within `t` of
/// the seed's.
fn seven_projection_flood(img: &RgbaImage, seed: (u32, u32), t: i32) -> Vec<bool> {
    let (w, h) = img.dimensions();
    let project = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0.map(i32::from);
        [
            p[0],
            p[1],
            p[2],
            p[3],
            p[0] - p[1],
            p[1] - p[2],
            p[0] - p[2],
        ]
    };
    let s = project(seed.0, seed.1);
    let near = |x: u32, y: u32| {
        project(x, y)
            .iter()
            .zip(&s)
            .all(|(a, b)| (a - b).abs() <= t)
    };
    let mut out = vec![false; (w * h) as usize];
    out[(seed.1 * w + seed.0) as usize] = true;
    let mut stack = vec![seed];
    while let Some((x, y)) = stack.pop() {
        let mut next = Vec::new();
        if x > 0 {
            next.push((x - 1, y));
        }
        if x + 1 < w {
            next.push((x + 1, y));
        }
        if y > 0 {
            next.push((x, y - 1));
        }
        if y + 1 < h {
            next.push((x, y + 1));
        }
        for (nx, ny) in next {
            let i = (ny * w + nx) as usize;
            if !out[i] && near(nx, ny) {
                out[i] = true;
                stack.push((nx, ny));
            }
        }
    }
    out
}

#[test]
fn the_worked_example() {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 255, 128])));
    // Was (191, 0, 64, 128): the W3C blend, then the alpha restored.
    let got = take(fill(
        &doc,
        &[255],
        [255, 0, 0, 255],
        BLEND_NORMAL,
        0.6,
        true,
    ));
    assert_eq!(got, [153, 0, 102, 128]);
}

#[test]
fn byte_opaque_and_transparent_pixels_match_the_unfrozen_path() {
    let base = semi(W, H);
    let doc = DrDocument::from_pixels(base.clone());
    let m = mask(W, H);
    let rgba = [INK[0], INK[1], INK[2], 255];
    for mode in [BLEND_NORMAL, BLEND_MULTIPLY, BLEND_SCREEN] {
        let frozen = take(fill(&doc, &m, rgba, mode, 0.7, true));
        let free = take(fill(&doc, &m, rgba, mode, 0.7, false));
        for i in 0..(W * H) as usize {
            let before = px(base.as_raw(), i);
            match before[3] {
                255 => assert_eq!(px(&frozen, i), px(&free, i), "opaque {i} mode {mode}"),
                0 => assert_eq!(px(&frozen, i), before, "transparent {i} mode {mode}"),
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------- 16-bit --

fn semi16(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let a = match i % 7 {
                0 => 0,
                1 => 65535,
                k => (k * 9001 + i * 13) as u16,
            };
            [
                (i * 2711 % 65536) as u16,
                (i * 977 % 65536) as u16,
                30000,
                a,
            ]
        })
        .collect()
}

fn coverage16(n: u32) -> Vec<u16> {
    (0..n)
        .map(|i| [0u16, 65535, 32768, 9000, 300, 50000][i as usize % 6])
        .collect()
}

fn samples16(d: &PrecisionDocument) -> Vec<[u16; 4]> {
    d.layer_source(0).unwrap().to_rgba16().unwrap()
}

const COLOR: [f32; 4] = [0.9, 0.2, 0.1, 1.0];

#[test]
fn sixteen_bit_paint_blend_flag_and_lock_follow_the_lerp() {
    let pixels = semi16(W, H);
    let d = native::doc(W, H, pixels.clone());
    let locked = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let cov = coverage16(W * H);
    let cs = [COLOR[0], COLOR[1], COLOR[2]].map(f64::from);
    for mode in [BLEND_NORMAL, BLEND_MULTIPLY, BLEND_SCREEN] {
        let request = |preserve: bool| {
            json!({"op":"paint_blend","layer":0,"color":COLOR,"color_space":"document",
                "opacity":0.75,"blend_mode":mode,"preserve_alpha":preserve})
        };
        let flag = samples16(&native::edit(&d, request(true), &cov).unwrap());
        let lock = samples16(&native::edit(&locked, request(false), &cov).unwrap());
        let free = samples16(&native::edit(&d, request(false), &cov).unwrap());
        for (i, before) in pixels.iter().enumerate() {
            let coverage = f64::from(cov[i]) / 65535.0;
            let sa = f64::from((f64::from(COLOR[3]) * 0.75 * coverage) as f32);
            let want = frozen16(*before, cs, sa, mode);
            near16(flag[i], want, &format!("flag mode {mode} pixel {i}"));
            near16(lock[i], want, &format!("lock mode {mode} pixel {i}"));
            match before[3] {
                65535 => assert_eq!(flag[i], free[i], "opaque {i} mode {mode}"),
                0 => assert_eq!(flag[i], *before, "transparent {i} mode {mode}"),
                _ => {}
            }
        }
    }
}

#[test]
fn sixteen_bit_brush_under_the_lock_follows_the_lerp() {
    let pixels = semi16(W, H);
    let d = native::doc(W, H, pixels.clone());
    let locked = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let cov = coverage16(W * H);
    let request =
        json!({"op":"paint","layer":0,"color":COLOR,"color_space":"document","opacity":0.75});
    let got = samples16(&native::edit(&locked, request, &cov).unwrap());
    let cs = [COLOR[0], COLOR[1], COLOR[2]].map(f64::from);
    for (i, before) in pixels.iter().enumerate() {
        let sa = f64::from(COLOR[3] * 0.75 * (f32::from(cov[i]) / 65535.0));
        near16(
            got[i],
            frozen16(*before, cs, sa, BLEND_NORMAL),
            &format!("brush {i}"),
        );
    }
}

// ----------------------------------------------------------------- float --

/// A float document holding one canvas-sized raster of `samples`, through a
/// float TIFF (`tests/float_fill_stroke.rs`' route).
fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("preserve.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), native::LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

fn samples32(d: &PrecisionDocument) -> Vec<[f32; 4]> {
    d.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn float_request(mode: c_int, preserve: bool) -> serde_json::Value {
    json!({"op":"paint_blend","layer":0,"color":[0.9,0.2,0.1,1.0],"color_space":"document",
        "opacity":0.75,"blend_mode":mode,"preserve_alpha":preserve})
}

#[test]
fn float_paint_blend_flag_and_lock_follow_the_lerp() {
    let samples: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let v = (i % 17) as f32 / 8.0 - 0.5;
            let a = [0.0, 1.0, 0.2, 0.6, 0.85][i as usize % 5];
            [v, 1.5 - v, 0.25 * v, a]
        })
        .collect();
    let d = float_document(W, H, &samples);
    let locked = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let cov: Vec<u16> = (0..W * H)
        .map(|i| [65535u16, 40000, 20000][i as usize % 3])
        .collect();
    // The decoded linear ink: a full-coverage opaque Normal fill shows it.
    let probe = float_document(1, 1, &[[0.3, 0.3, 0.3, 1.0]]);
    let opaque = json!({"op":"paint_blend","layer":0,"color":[0.9,0.2,0.1,1.0],
        "color_space":"document","opacity":1.0,"blend_mode":0});
    let ink = samples32(&native::edit(&probe, opaque, &[65535]).unwrap())[0];
    let cs = [ink[0], ink[1], ink[2]].map(f64::from);
    for mode in [BLEND_NORMAL, BLEND_MULTIPLY, BLEND_DARKEN] {
        let flag = samples32(&native::edit(&d, float_request(mode, true), &cov).unwrap());
        let lock = samples32(&native::edit(&locked, float_request(mode, false), &cov).unwrap());
        let free = samples32(&native::edit(&d, float_request(mode, false), &cov).unwrap());
        for (i, before) in samples.iter().enumerate() {
            let sa = f64::from((0.75 * f64::from(cov[i]) / 65535.0) as f32);
            for (got, what) in [(flag[i], "flag"), (lock[i], "lock")] {
                assert_eq!(
                    got[3].to_bits(),
                    before[3].to_bits(),
                    "{what} {i}: alpha kept"
                );
                if before[3] == 0.0 {
                    assert_eq!(
                        got.map(f32::to_bits),
                        before.map(f32::to_bits),
                        "{what} {i}"
                    );
                    continue;
                }
                for c in 0..3 {
                    let cb = f64::from(before[c]);
                    let want = cb + sa * (blend(mode, cb, cs[c]) - cb);
                    let err = (f64::from(got[c]) - want).abs();
                    assert!(
                        err <= 1e-6 * want.abs().max(1.0),
                        "{what} mode {mode} pixel {i} channel {c}: {} vs {want}",
                        got[c]
                    );
                }
            }
            if before[3] == 1.0 {
                assert_eq!(
                    flag[i].map(f32::to_bits),
                    free[i].map(f32::to_bits),
                    "opaque {i}"
                );
            }
        }
    }
}
