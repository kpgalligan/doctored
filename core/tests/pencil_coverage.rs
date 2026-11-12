//! The Pencil's aliased tip (`brush_coverage`, THE ALIASED TIP) and the byte
//! overlay it commits through (`dr_brush_overlay_rgba8`), black-box through the
//! C ABI against an oracle written here from the rule alone: the test's own
//! ellipse frame, its own 8×8 sample count (inside when the normalized squared
//! distance is at most 1; 32 of 64 or more paints), its own pixel-mode square
//! overlap, its own source-over accumulation and one rounding, and its own
//! premultiplication. Then the coverage through `paint_blend` on a 16-bit and a
//! 32-bit float document and `dr_doc_painting_layer` on an 8-bit one: every
//! pixel is either exactly the ink or exactly the base, painted where the
//! oracle says.
use crate::common::{doc_from, layer_pixels, COMPOSITE_OVER};
use crate::float_fixture::float_doc;
use crate::native_scalar::{doc, edit, error, text, LIMIT};
use doctored_core::brush_coverage::band_bound;
use doctored_core::ffi_brush::*;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_painting_layer};
use doctored_core::icc::IccProfile;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ptr;

fn request(roundness: f64, angle: f64, pixel: bool) -> Value {
    json!({"version":1,"hardness":1.,"roundness":roundness,"angle_degrees":angle,
           "pixel":pixel,"aliased":true})
}

/// `dr_brush_u16`, or its refusal.
fn coverage(w: u32, h: u32, request: &Value, dabs: &[f64]) -> Result<Vec<u16>, String> {
    let t = text(request.clone());
    let mut out = vec![777; (w * h) as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16(
            w,
            h,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if ok {
        Ok(out)
    } else {
        Err(unsafe { error(e) })
    }
}

/// `dr_brush_overlay_rgba8`, or its refusal.
fn overlay(
    (w, h): (u32, u32),
    request: &Value,
    dabs: &[f64],
    rgb: [u8; 3],
    selection: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let t = text(request.clone());
    let mut out = vec![7; (w * h * 4) as usize];
    let mut e = ptr::null_mut();
    let (mask, mask_len) = selection.map_or((ptr::null(), 0), |s| (s.as_ptr(), s.len()));
    let ok = unsafe {
        dr_brush_overlay_rgba8(
            w,
            h,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            rgb.as_ptr(),
            mask,
            mask_len,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if ok {
        Ok(out)
    } else {
        Err(unsafe { error(e) })
    }
}

/// The oracle's coverage of pixel (x, y) by one dab, before its deposit: 1 or 0.
fn oracle_pixel(dab: &[f64], roundness: f64, angle: f64, pixel: bool, x: usize, y: usize) -> f64 {
    let (cx, cy, r) = (dab[0], dab[1], dab[2] / 2.);
    let area = if pixel {
        let side = |p: f64, c: f64| ((p + 1.).min(c + r) - p.max(c - r)).clamp(0., 1.);
        side(x as f64, cx) * side(y as f64, cy)
    } else {
        let (s, c) = angle.to_radians().sin_cos();
        let mut inside = 0;
        for sy in 0..8 {
            for sx in 0..8 {
                let dx = x as f64 + (2 * sx + 1) as f64 / 16. - cx;
                let dy = y as f64 + (2 * sy + 1) as f64 / 16. - cy;
                // The major axis rotated by +angle counter-clockwise on a y-down canvas.
                let (u, v) = (dx * c - dy * s, dx * s + dy * c);
                let (a, b) = (r, r * roundness);
                if u * u / (a * a) + v * v / (b * b) <= 1. {
                    inside += 1;
                }
            }
        }
        f64::from(inside) / 64.
    };
    if area >= 0.5 {
        1.
    } else {
        0.
    }
}

/// The oracle's whole stroke: source-over in dab order, rounded once at `opacity`.
fn oracle(w: u32, h: u32, roundness: f64, angle: f64, pixel: bool, dabs: &[f64]) -> Vec<u16> {
    let mut field = vec![0_f64; (w * h) as usize];
    for dab in dabs.chunks_exact(4) {
        for y in 0..h as usize {
            for x in 0..w as usize {
                let c = oracle_pixel(dab, roundness, angle, pixel, x, y) * dab[3];
                let old = &mut field[y * w as usize + x];
                *old += c * (1. - *old);
            }
        }
    }
    field.iter().map(|v| (v * 65535.).round() as u16).collect()
}

/// One oracle case: canvas, roundness, angle, pixel mode, dabs.
type Case = ((u32, u32), f64, f64, bool, Vec<f64>);

#[test]
fn aliased_dabs_match_an_independent_half_coverage_oracle() {
    let cases: Vec<Case> = vec![
        // Odd diameters at pixel centres, even ones at pixel corners (the snapped Pencil).
        ((21, 21), 1., 0., false, vec![10.5, 10.5, 7., 1.]),
        ((21, 21), 1., 0., false, vec![10., 10., 8., 1.]),
        ((40, 40), 1., 0., false, vec![20.5, 20.5, 33., 1.]),
        // Sub-pixel centres, a dab below one pixel and one clipped by every edge.
        (
            (17, 13),
            1.,
            0.,
            false,
            vec![6.3, 5.71, 6.6, 1., 9.13, 2.2, 0.8, 1.],
        ),
        (
            (9, 7),
            1.,
            0.,
            false,
            vec![-1.2, 3.4, 9., 1., 8.6, 6.9, 5., 1.],
        ),
        // A rotated narrow ellipse.
        ((25, 25), 0.4, 30., false, vec![12.5, 12.5, 19., 1.]),
        // Deposits below one, overlapping, and the source-over between them.
        (
            (24, 12),
            1.,
            0.,
            false,
            vec![6., 6., 8., 0.37, 10., 6., 8., 0.61, 14.5, 5.5, 9., 0.83],
        ),
        // Pixel mode: half of the square or more.
        (
            (6, 3),
            1.,
            0.,
            true,
            vec![1.5, 1.5, 1., 1., 3.2, 1.5, 1., 1., 4.9, 1.5, 1., 0.5],
        ),
    ];
    for ((w, h), roundness, angle, pixel, dabs) in cases {
        let got = coverage(w, h, &request(roundness, angle, pixel), &dabs).unwrap();
        let want = oracle(w, h, roundness, angle, pixel, &dabs);
        assert_eq!(
            got, want,
            "{w}x{h} roundness {roundness} angle {angle} pixel {pixel}"
        );
    }
}

#[test]
fn a_deposit_one_dab_is_two_valued_symmetric_and_within_its_band_of_the_disc() {
    for (n, centre, diameter) in [(31_u32, 15.5, 9.), (32, 16., 12.), (61, 30.5, 41.)] {
        let out = coverage(
            n,
            n,
            &request(1., 0., false),
            &[centre, centre, diameter, 1.],
        )
        .unwrap();
        assert!(
            out.iter().all(|&v| v == 0 || v == 65535),
            "diameter {diameter}"
        );
        let at = |x: usize, y: usize| out[y * n as usize + x];
        let last = n as usize - 1;
        for y in 0..n as usize {
            for x in 0..n as usize {
                assert_eq!(at(x, y), at(last - x, y), "mirror x at {x},{y}");
                assert_eq!(at(x, y), at(x, last - y), "mirror y at {x},{y}");
                assert_eq!(at(x, y), at(y, x), "transpose at {x},{y}");
            }
        }
        let painted = out.iter().filter(|&&v| v == 65535).count() as f64;
        let disc = std::f64::consts::PI * diameter * diameter / 4.;
        assert!(
            (painted - disc).abs() <= band_bound(diameter) as f64,
            "{painted} vs {disc}"
        );
    }
}

#[test]
fn the_aliased_key_refuses_a_soft_tip_and_its_absence_keeps_every_byte() {
    let mut soft = request(1., 0., false);
    soft["hardness"] = json!(0.5);
    let refusal = coverage(4, 4, &soft, &[2., 2., 3., 1.]).unwrap_err();
    assert!(
        refusal.contains("native brush aliased tip requires hardness 1"),
        "{refusal}"
    );
    let dabs = [6.3, 5.71, 6.6, 0.7, 7.5, 6., 5., 1.];
    let mut plain = request(1., 0., false);
    plain.as_object_mut().unwrap().remove("aliased");
    let mut off = request(1., 0., false);
    off["aliased"] = json!(false);
    let without = coverage(15, 12, &plain, &dabs).unwrap();
    assert_eq!(coverage(15, 12, &off, &dabs).unwrap(), without);
    assert!(without.iter().any(|&v| v != 0 && v != 65535 && v != 45875));
}

/// The oracle's premultiplied overlay of `coverage`.
fn oracle_overlay(coverage: &[u16], ink: [u8; 3], selection: Option<&[u8]>) -> Vec<u8> {
    let half_up = |x: f64| (x + 0.5).floor();
    let mut out = Vec::new();
    for (i, &c) in coverage.iter().enumerate() {
        let mut a = half_up(f64::from(c) * 255. / 65535.);
        if let Some(s) = selection {
            a = half_up(a * f64::from(s[i]) / 255.);
        }
        for v in ink {
            out.push(half_up(f64::from(v) * a / 255.) as u8);
        }
        out.push(a as u8);
    }
    out
}

#[test]
fn the_byte_overlay_premultiplies_and_paints_only_ink_or_base() {
    let (w, h) = (23_u32, 17_u32);
    let dabs = [7.5, 8.5, 9., 1., 15., 9., 10., 1.];
    let ink = [201, 37, 118];
    let cov = coverage(w, h, &request(1., 0., false), &dabs).unwrap();
    let plain = overlay((w, h), &request(1., 0., false), &dabs, ink, None).unwrap();
    assert_eq!(plain, oracle_overlay(&cov, ink, None));
    let selection: Vec<u8> = (0..w * h).map(|i| ((i * 37) % 256) as u8).collect();
    let masked = overlay(
        (w, h),
        &request(1., 0., false),
        &dabs,
        ink,
        Some(&selection),
    )
    .unwrap();
    assert_eq!(masked, oracle_overlay(&cov, ink, Some(&selection)));
    // A partial deposit's overlay is the same rule over partial coverage.
    let mut faint = request(1., 0., false);
    faint["opacity"] = json!(0.43);
    let partial = coverage(w, h, &faint, &dabs).unwrap();
    assert_eq!(
        overlay((w, h), &faint, &dabs, ink, None).unwrap(),
        oracle_overlay(&partial, ink, None)
    );
    // Onto an opaque 8-bit layer: each pixel is the ink or the base, as the oracle paints.
    let base = Rgba([12, 140, 250, 255]);
    let dir = tempfile::TempDir::new().unwrap();
    let doc = doc_from(&dir, "pencil.png", &RgbaImage::from_pixel(w, h, base));
    let painted =
        unsafe { dr_doc_painting_layer(doc, 0, plain.as_ptr(), w, h, COMPOSITE_OVER, 1.) };
    assert!(!painted.is_null());
    let pixels = layer_pixels(painted, 0);
    let want = oracle(w, h, 1., 0., false, &dabs);
    for (i, px) in pixels.chunks_exact(4).enumerate() {
        let expected = if want[i] == 65535 {
            [ink[0], ink[1], ink[2], 255]
        } else {
            base.0
        };
        assert_eq!(px, expected, "pixel {i}");
    }
    unsafe {
        dr_doc_free(painted);
        dr_doc_free(doc);
    }
}

#[test]
fn the_overlay_refuses_bad_lengths_and_null_ink_leaving_output_untouched() {
    let t = text(request(1., 0., false));
    let dabs = [1.5, 1.5, 3., 1.];
    let ink = [1_u8, 2, 3];
    let mask = [255_u8; 9];
    for (rgb, sel, sel_len, out_len) in [
        (ptr::null(), ptr::null(), 0, 36),
        (ink.as_ptr(), mask.as_ptr(), 8, 36),
        (ink.as_ptr(), ptr::null(), 9, 36),
        (ink.as_ptr(), ptr::null(), 0, 35),
    ] {
        let mut out = [7_u8; 36];
        let mut e = ptr::null_mut();
        let ok = unsafe {
            dr_brush_overlay_rgba8(
                3,
                3,
                t.as_ptr(),
                dabs.as_ptr(),
                1,
                rgb,
                sel,
                sel_len,
                out.as_mut_ptr(),
                out_len,
                LIMIT,
                0,
                ptr::null(),
                &mut e,
            )
        };
        assert!(!ok);
        assert!(!unsafe { error(e) }.is_empty());
        assert_eq!(out, [7; 36]);
    }
}

#[test]
fn sixteen_bit_and_float_documents_take_exactly_the_ink_or_the_base() {
    let (w, h) = (19_u32, 14_u32);
    let dabs = [6.5, 7.5, 9., 1., 12., 6., 8., 1.];
    let cov = coverage(w, h, &request(1., 0., false), &dabs).unwrap();
    let want = oracle(w, h, 1., 0., false, &dabs);
    assert_eq!(cov, want);
    let paint = json!({"op":"paint_blend","layer":0,"color":[0.81,0.22,0.47,1.],
                       "color_space":"document","opacity":1.,"blend_mode":0});
    // 16 bits: the ink is what a full-coverage pixel takes.
    let base16 = [9001_u16, 30003, 52005, 65535];
    let ink16 = edit(&doc(1, 1, vec![base16]), paint.clone(), &[65535]).unwrap();
    let ink16 = ink16.layer_source(0).unwrap().to_rgba16().unwrap()[0];
    let d = doc(w, h, vec![base16; (w * h) as usize]);
    let out = edit(&d, paint.clone(), &cov).unwrap();
    for (i, px) in out
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(
            *px,
            if want[i] == 65535 { ink16 } else { base16 },
            "16-bit pixel {i}"
        );
    }
    // 32-bit float: the same, bit for bit.
    let srgb = IccProfile::srgb();
    let base32 = [0.25_f32, -0.5, 3.0, 1.0];
    let one = float_doc(1, 1, &srgb, &[vec![base32]]);
    let ink32 = edit(&one, paint.clone(), &[65535]).unwrap();
    let ink32 = ink32.layer_source(0).unwrap().to_rgba32().unwrap()[0].map(f32::to_bits);
    let d = float_doc(w, h, &srgb, &[vec![base32; (w * h) as usize]]);
    let out = edit(&d, paint, &cov).unwrap();
    for (i, px) in out
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap()
        .iter()
        .enumerate()
    {
        let want_bits = if want[i] == 65535 {
            ink32
        } else {
            base32.map(f32::to_bits)
        };
        assert_eq!(px.map(f32::to_bits), want_bits, "float pixel {i}");
    }
}
