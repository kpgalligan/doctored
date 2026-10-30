//! Filter ▸ Neural Filters ▸ Colorize (M4 phase 2) through the FFI:
//! the model input (`dr_*_neural_colorize_input`), the adjusted chroma
//! carried onto the layer's own L* (`dr_*_neural_colorize_apply`) and the
//! plan, at 8 and 16 bits, with STUB chroma planes in place of DDColor.
//!
//! # The oracles
//!
//! Written here from the header's contract, in binary64. A pixel's Lab is
//! read through the document profile's own public model
//! (`MatrixTrc::to_pcs_xyz64`, `icc_lut::xyz_to_lab`): the same readout on
//! the source and the output, so "L* kept" is the reconstruction's claim
//! alone. The chroma upscale is bilinear at `s = (o + 0.5)·side/dim` with
//! taps clamped to the plane; Saturation is `ab·(1 + s/50)`; a balance adds
//! `(v/50)·30` along the unit a*b* of the named sRGB primary; Color Artifact
//! Reduction is a normalized Gaussian of σ = 4·r/100 model pixels, which
//! keeps a constant everywhere and a linear ramp wherever its taps stay
//! inside the plane.
//!
//! # Bounds, declared before the first run
//!
//! * L*: |ΔL*| ≤ 0.5 at 8 bits (half a code per channel), ≤ 0.02 at 16.
//! * Alpha: bit-exact.
//! * a*, b* against the closed forms at 16 bits: ±0.1.
//! * The gamut map's hue: ≤ 0.5°.
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_neural::*;
use doctored_core::ffi_precision::dr_precision_image_free;
use doctored_core::icc::IccProfile;
use doctored_core::icc_lut::xyz_to_lab;
use doctored_core::icc_transform::{MatrixTrc, PrecisionToneEncoder};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;

use crate::cmyk_fixture::press_profile;
use crate::common::{img_pixels, take_err_string};

const LIMIT: u64 = 2 << 30;
const SRGB: i32 = 0;
const DISPLAY_P3: i32 = 1;
const PROPHOTO: i32 = 3;

fn builtin(which: i32) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    let mut bytes = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, bytes.as_mut_ptr(), len) });
    bytes
}

fn model(profile: &[u8]) -> MatrixTrc {
    IccProfile::parse(profile)
        .expect("profile")
        .model()
        .expect("matrix/TRC")
        .clone()
}

fn lab(model: &MatrixTrc, rgb: [f64; 3]) -> [f64; 3] {
    xyz_to_lab(model.to_pcs_xyz64(rgb))
}

fn lab8(model: &MatrixTrc, p: &[u8]) -> [f64; 3] {
    lab(model, [0, 1, 2].map(|c| f64::from(p[c]) / 255.0))
}

fn lab16(model: &MatrixTrc, p: [u16; 4]) -> [f64; 3] {
    lab(model, [0, 1, 2].map(|c| f64::from(p[c]) / 65535.0))
}

/// A varied 8-bit layer: colours over the cube, alpha varied.
fn varied8(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            out.extend([
                ((x * 255) / (w - 1)) as u8,
                ((y * 255) / (h - 1)) as u8,
                ((i * 97) % 256) as u8,
                (255 - (i * 13) % 200) as u8,
            ]);
        }
    }
    out
}

fn varied16(w: u32, h: u32) -> Vec<[u16; 4]> {
    varied8(w, h)
        .chunks(4)
        .enumerate()
        .map(|(i, p)| {
            let j = (i as u16).wrapping_mul(37) & 0xff;
            [
                (u16::from(p[0]) * 257) ^ j,
                (u16::from(p[1]) * 257) ^ (j >> 1),
                u16::from(p[2]) * 257,
                (u16::from(p[3]) * 257).saturating_sub(j),
            ]
        })
        .collect()
}

/// A smooth stub chroma plane: a* and b* vary over the plane.
fn swirl(side: usize, amplitude: f32) -> Vec<f32> {
    let mut out = vec![0.0; 2 * side * side];
    for y in 0..side {
        for x in 0..side {
            let t = (x + 2 * y) as f32 / side as f32;
            out[y * side + x] = amplitude * (3.0 * t).cos();
            out[side * side + y * side + x] = amplitude * (2.0 * t).sin();
        }
    }
    out
}

fn uniform(side: usize, a: f32, b: f32) -> Vec<f32> {
    let mut out = vec![a; 2 * side * side];
    out[side * side..].fill(b);
    out
}

fn json(text: &str) -> CString {
    CString::new(text).expect("json")
}

struct Byte(*mut DrImage);
impl Drop for Byte {
    fn drop(&mut self) {
        unsafe { dr_image_free(self.0) };
    }
}

fn byte(w: u32, h: u32, pixels: &[u8]) -> Byte {
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!image.is_null());
    Byte(image)
}

fn wide(w: u32, h: u32, pixels: Vec<[u16; 4]>, profile: &[u8]) -> PrecisionImage {
    let profile = IccProfile::parse(profile).expect("profile");
    PrecisionImage::from_rgba16(w, h, pixels, std::sync::Arc::new(profile), None).unwrap()
}

fn input8(image: &Byte, profile: &[u8], side: u32, count: usize) -> Result<Vec<f32>, String> {
    let mut out = vec![0.0_f32; count];
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_image_neural_colorize_input(
            image.0,
            profile.as_ptr(),
            profile.len(),
            side,
            out.as_mut_ptr(),
            count,
            LIMIT,
            0,
            ptr::null(),
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

fn input16(image: &PrecisionImage, profile: &[u8], side: u32) -> Result<Vec<f32>, String> {
    let count = 3 * side as usize * side as usize;
    let mut out = vec![0.0_f32; count];
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_image_neural_colorize_input(
            image,
            profile.as_ptr(),
            profile.len(),
            side,
            out.as_mut_ptr(),
            count,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

#[allow(clippy::too_many_arguments)]
fn apply8_with(
    image: &Byte,
    profile: &[u8],
    side: u32,
    chroma: &[f32],
    adjust: &str,
    limit: u64,
    control: *const doctored_core::job_control::DrJobControl,
) -> Result<Option<Vec<u8>>, String> {
    let adjust = json(adjust);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_image_neural_colorize_apply(
            image.0,
            profile.as_ptr(),
            profile.len(),
            side,
            chroma.as_ptr(),
            chroma.len(),
            adjust.as_ptr(),
            limit,
            0,
            control,
            &mut err,
        )
    };
    if out.is_null() {
        return if err.is_null() {
            Ok(None)
        } else {
            Err(take_err_string(err))
        };
    }
    let pixels = img_pixels(out);
    unsafe { dr_image_free(out) };
    Ok(Some(pixels))
}

fn apply8(
    image: &Byte,
    profile: &[u8],
    side: u32,
    chroma: &[f32],
    adjust: &str,
) -> Result<Option<Vec<u8>>, String> {
    apply8_with(image, profile, side, chroma, adjust, LIMIT, ptr::null())
}

fn apply16(
    image: &PrecisionImage,
    profile: &[u8],
    side: u32,
    chroma: &[f32],
    adjust: &str,
) -> Result<Option<Vec<[u16; 4]>>, String> {
    let adjust = json(adjust);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_neural_colorize_apply(
            image,
            profile.as_ptr(),
            profile.len(),
            side,
            chroma.as_ptr(),
            chroma.len(),
            adjust.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return if err.is_null() {
            Ok(None)
        } else {
            Err(take_err_string(err))
        };
    }
    let pixels = unsafe { &*out }.to_rgba16().unwrap();
    unsafe { dr_precision_image_free(out) };
    Ok(Some(pixels))
}

/// The bilinear oracle: the plane sampled at output `(x, y)` of `w × h`.
fn bilinear(plane: &[f32], side: usize, w: u32, h: u32, x: u32, y: u32) -> [f64; 2] {
    let axis = |o: u32, dim: u32| {
        let s = (f64::from(o) + 0.5) * side as f64 / f64::from(dim) - 0.5;
        let base = s.floor();
        let frac = s - base;
        let clamp = |i: f64| (i as i64).clamp(0, side as i64 - 1) as usize;
        ([clamp(base), clamp(base + 1.0)], [1.0 - frac, frac])
    };
    let (xs, wx) = axis(x, w);
    let (ys, wy) = axis(y, h);
    let mut out = [0.0; 2];
    for (yi, &wyv) in ys.iter().zip(&wy) {
        for (xi, &wxv) in xs.iter().zip(&wx) {
            for c in 0..2 {
                out[c] += wyv * wxv * f64::from(plane[c * side * side + yi * side + xi]);
            }
        }
    }
    out
}

#[test]
fn lightness_and_alpha_are_kept_in_each_profile() {
    let (w, h, side) = (37u32, 23u32, 16u32);
    let chroma = swirl(side as usize, 45.0);
    for which in [SRGB, DISPLAY_P3, PROPHOTO] {
        let profile = builtin(which);
        let m = model(&profile);
        let source = varied8(w, h);
        let image = byte(w, h, &source);
        let out = apply8(&image, &profile, side, &chroma, "{}")
            .unwrap()
            .expect("changed");
        let mut worst: f64 = 0.0;
        for (s, o) in source.chunks(4).zip(out.chunks(4)) {
            worst = worst.max((lab8(&m, s)[0] - lab8(&m, o)[0]).abs());
            assert_eq!(s[3], o[3], "alpha");
        }
        assert!(worst <= 0.5, "profile {which}: 8-bit |ΔL*| {worst}");

        let pixels = varied16(w, h);
        let image = wide(w, h, pixels.clone(), &profile);
        let out = apply16(&image, &profile, side, &chroma, "{}")
            .unwrap()
            .expect("changed");
        let mut worst: f64 = 0.0;
        for (&s, &o) in pixels.iter().zip(&out) {
            worst = worst.max((lab16(&m, s)[0] - lab16(&m, o)[0]).abs());
            assert_eq!(s[3], o[3], "alpha");
        }
        assert!(worst <= 0.02, "profile {which}: 16-bit |ΔL*| {worst}");
    }
}

#[test]
fn input_is_the_neutral_gray_of_each_cells_lightness() {
    let profile = builtin(DISPLAY_P3);
    let m = model(&profile);
    let side = 16u32;
    let cells = (side * side) as usize;
    // A uniform layer: every cell is that colour's L*, as an sRGB neutral.
    let colour = [180u8, 60, 90, 255];
    let image = byte(20, 12, &colour.repeat(20 * 12));
    let out = input8(&image, &profile, side, 3 * cells).unwrap();
    let l = lab8(&m, &colour)[0];
    let fy = (l + 16.0) / 116.0;
    let y = if fy > 6.0 / 29.0 {
        fy.powi(3)
    } else {
        (fy - 16.0 / 116.0) * 3.0 * (6.0f64 / 29.0).powi(2)
    };
    let gray = if y <= 0.0031308 {
        12.92 * y
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    };
    for i in 0..cells {
        for c in 0..3 {
            assert!(
                (f64::from(out[c * cells + i]) - gray).abs() <= 1e-3,
                "cell {i} channel {c}: {} against {gray}",
                out[c * cells + i]
            );
        }
    }
    // A horizontal ramp at 16 bits: neutral cells, rising left to right.
    let (w, h) = (64u32, 8u32);
    let ramp: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let v = ((i % w) * 1000) as u16;
            [v, v, v, 65535]
        })
        .collect();
    let out = input16(&wide(w, h, ramp, &profile), &profile, side).unwrap();
    for y in 0..side as usize {
        for x in 0..side as usize {
            let i = y * side as usize + x;
            assert!(
                (out[i] - out[cells + i]).abs() <= 1e-3
                    && (out[i] - out[2 * cells + i]).abs() <= 1e-3
            );
            if x > 0 {
                assert!(out[i] > out[i - 1], "the ramp rises");
            }
        }
    }
}

#[test]
fn chroma_upscale_matches_the_bilinear_oracle() {
    let profile = builtin(SRGB);
    let m = model(&profile);
    let (w, h, side) = (53u32, 41u32, 16usize);
    let chroma = swirl(side, 20.0);
    // A mid gray: every sampled chroma stays inside sRGB.
    let gray = [30000u16, 30000, 30000, 65535];
    let image = wide(w, h, vec![gray; (w * h) as usize], &profile);
    let out = apply16(&image, &profile, side as u32, &chroma, "{}")
        .unwrap()
        .unwrap();
    for y in 0..h {
        for x in 0..w {
            let got = lab16(&m, out[(y * w + x) as usize]);
            let want = bilinear(&chroma, side, w, h, x, y);
            for c in 0..2 {
                assert!(
                    (got[c + 1] - want[c]).abs() <= 0.1,
                    "({x}, {y}) channel {c}: {} against {}",
                    got[c + 1],
                    want[c]
                );
            }
        }
    }
}

/// The 16-bit mid-gray layer's ab under `adjust` with a uniform stub plane.
fn uniform_ab(adjust: &str, a: f32, b: f32) -> [f64; 2] {
    let profile = builtin(SRGB);
    let m = model(&profile);
    let side = 16u32;
    let gray = [30000u16, 30000, 30000, 65535];
    let image = wide(8, 8, vec![gray; 64], &profile);
    // Unchanged (saturation −50 on a neutral) answers NULL: the gray itself.
    let out = apply16(
        &image,
        &profile,
        side,
        &uniform(side as usize, a, b),
        adjust,
    )
    .unwrap()
    .unwrap_or_else(|| vec![gray; 64]);
    let l = lab16(&m, out[27]);
    [l[1], l[2]]
}

fn primary_direction(rgb: [f64; 3]) -> [f64; 2] {
    let srgb = IccProfile::srgb();
    let lab = lab(srgb.model().unwrap(), rgb);
    let length = lab[1].hypot(lab[2]);
    [lab[1] / length, lab[2] / length]
}

#[test]
fn each_adjustment_matches_its_closed_form() {
    let (a, b) = (10.0, -5.0);
    let close = |got: [f64; 2], want: [f64; 2], what: &str| {
        for c in 0..2 {
            assert!(
                (got[c] - want[c]).abs() <= 0.1,
                "{what}: {got:?} against {want:?}"
            );
        }
    };
    close(uniform_ab("{}", a as f32, b as f32), [a, b], "none");
    for s in [-50.0, -20.0, 25.0, 50.0] {
        let k = 1.0 + s / 50.0;
        close(
            uniform_ab(&format!("{{\"saturation\":{s}}}"), a as f32, b as f32),
            [a * k, b * k],
            "saturation",
        );
    }
    for (key, rgb) in [
        ("cyan_red", [1.0, 0.0, 0.0]),
        ("magenta_green", [0.0, 1.0, 0.0]),
        ("yellow_blue", [0.0, 0.0, 1.0]),
    ] {
        let u = primary_direction(rgb);
        for v in [-50.0, -10.0, 35.0] {
            let push = v / 50.0 * 30.0;
            close(
                uniform_ab(&format!("{{\"{key}\":{v}}}"), a as f32, b as f32),
                [a + push * u[0], b + push * u[1]],
                key,
            );
        }
    }
    // Saturation applies before the balance: (ab·k) + push.
    let u = primary_direction([1.0, 0.0, 0.0]);
    close(
        uniform_ab("{\"saturation\":-25,\"cyan_red\":20}", a as f32, b as f32),
        [a * 0.5 + 12.0 * u[0], b * 0.5 + 12.0 * u[1]],
        "saturation then balance",
    );
    // A constant plane is a fixed point of the artifact blur.
    close(
        uniform_ab("{\"color_artifact_reduction\":100}", a as f32, b as f32),
        [a, b],
        "artifact on a constant",
    );
}

#[test]
fn artifact_reduction_keeps_interior_ramps_and_smooths_noise() {
    let profile = builtin(SRGB);
    let m = model(&profile);
    let side = 64usize;
    // a* ramps along x, b* is constant; one output pixel per model cell.
    let mut plane = vec![0.0f32; 2 * side * side];
    for y in 0..side {
        for x in 0..side {
            plane[y * side + x] = (x as f32 - 32.0) * 0.5;
            plane[side * side + y * side + x] = 4.0;
        }
    }
    let gray = [30000u16, 30000, 30000, 65535];
    let image = wide(side as u32, side as u32, vec![gray; side * side], &profile);
    let out = apply16(
        &image,
        &profile,
        side as u32,
        &plane,
        "{\"color_artifact_reduction\":100}",
    )
    .unwrap()
    .unwrap();
    // σ 4 reaches 12 cells: the interior keeps the ramp.
    for y in 12..side - 12 {
        for x in 12..side - 12 {
            let got = lab16(&m, out[y * side + x]);
            assert!(
                (got[1] - f64::from(plane[y * side + x])).abs() <= 0.1,
                "({x}, {y}) a*"
            );
            assert!((got[2] - 4.0).abs() <= 0.1, "({x}, {y}) b*");
        }
    }
    // A checkerboard of ±10 a* is flattened towards 0.
    let mut checker = vec![0.0f32; 2 * side * side];
    for y in 0..side {
        for x in 0..side {
            checker[y * side + x] = if (x + y) % 2 == 0 { 10.0 } else { -10.0 };
        }
    }
    let out = apply16(
        &image,
        &profile,
        side as u32,
        &checker,
        "{\"color_artifact_reduction\":50}",
    )
    .unwrap()
    .unwrap();
    let got = lab16(&m, out[32 * side + 32]);
    assert!(got[1].abs() <= 0.5, "checker a* {}", got[1]);
}

#[test]
fn the_gamut_map_keeps_hue_and_lightness() {
    for which in [SRGB, PROPHOTO] {
        let profile = builtin(which);
        let m = model(&profile);
        let side = 16u32;
        // A dark neutral at 16 bits, the chroma far outside either gamut.
        let gray = [12000u16, 12000, 12000, 65535];
        let source_l = lab16(&m, gray)[0];
        let image = wide(16, 16, vec![gray; 256], &profile);
        let (a, b) = (300.0f32, 220.0f32);
        let out = apply16(&image, &profile, side, &uniform(side as usize, a, b), "{}")
            .unwrap()
            .unwrap();
        let got = lab16(&m, out[100]);
        let hue = got[2].atan2(got[1]).to_degrees();
        let want = f64::from(b).atan2(f64::from(a)).to_degrees();
        assert!(
            (hue - want).abs() <= 0.5,
            "profile {which}: hue {hue} against {want}"
        );
        assert!(
            (got[0] - source_l).abs() <= 0.02,
            "profile {which}: L* {} against {source_l}",
            got[0]
        );
        let chroma = got[1].hypot(got[2]);
        assert!(
            chroma > 1.0 && chroma < f64::from(a).hypot(f64::from(b)),
            "chroma {chroma}"
        );
        if which == SRGB {
            // Mapped to the boundary: some channel sits at a code end.
            let p = out[100];
            assert!(p[..3].iter().any(|&v| v <= 64 || v >= 65471), "{p:?}");
        }
    }
}

#[test]
fn stub_planes_neutral_unchanged_and_not_finite() {
    let profile = builtin(SRGB);
    let side = 16u32;
    let cells = (side * side) as usize;
    // A gray layer under a zero plane changes nothing: NULL, no error.
    let gray = [120u8, 120, 120, 200].repeat(9 * 7);
    let image = byte(9, 7, &gray);
    assert_eq!(
        apply8(&image, &profile, side, &vec![0.0; 2 * cells], "{}").unwrap(),
        None
    );
    let wide_gray = wide(9, 7, vec![[30000, 30000, 30000, 4000]; 63], &profile);
    assert_eq!(
        apply16(&wide_gray, &profile, side, &vec![0.0; 2 * cells], "{}").unwrap(),
        None
    );
    // A coloured layer under a zero plane becomes the neutral of its own L*.
    let m = model(&profile);
    let colour = [200u8, 80, 40, 255];
    let image = byte(4, 4, &colour.repeat(16));
    let out = apply8(&image, &profile, side, &vec![0.0; 2 * cells], "{}")
        .unwrap()
        .unwrap();
    let got = lab8(&m, &out[..4]);
    assert!((got[0] - lab8(&m, &colour)[0]).abs() <= 0.5);
    assert!(got[1].abs() <= 1.0 && got[2].abs() <= 1.0, "{got:?}");
    // Saturation −50 makes any prediction neutral.
    let out = apply8(
        &image,
        &profile,
        side,
        &swirl(16, 60.0),
        "{\"saturation\":-50}",
    )
    .unwrap()
    .unwrap();
    for p in out.chunks(4) {
        let l = lab8(&m, p);
        assert!(l[1].abs() <= 1.5 && l[2].abs() <= 1.5, "{l:?}");
    }
    // A non-finite plane refuses.
    let mut nan = vec![0.0f32; 2 * cells];
    nan[17] = f32::NAN;
    assert_eq!(
        apply8(&image, &profile, side, &nan, "{}").unwrap_err(),
        "the colorize model's output is not finite"
    );
}

#[test]
fn refusals_by_name() {
    let srgb = builtin(SRGB);
    let side = 16u32;
    let cells = (side * side) as usize;
    let image = byte(4, 4, &[100u8, 50, 25, 255].repeat(16));
    let zero = vec![0.0f32; 2 * cells];
    let refuse = |adjust: &str| apply8(&image, &srgb, side, &zero, adjust).unwrap_err();
    assert!(refuse("{\"vibrance\":1}").starts_with("invalid colorize adjustments: "));
    assert!(refuse("[1]").starts_with("invalid colorize adjustments: "));
    assert_eq!(
        refuse("{\"saturation\":60}"),
        "colorize saturation must be -50 to 50"
    );
    assert_eq!(
        refuse("{\"cyan_red\":-51}"),
        "colorize cyan_red must be -50 to 50"
    );
    assert_eq!(
        refuse("{\"magenta_green\":50.5}"),
        "colorize magenta_green must be -50 to 50"
    );
    assert_eq!(
        refuse("{\"yellow_blue\":-90}"),
        "colorize yellow_blue must be -50 to 50"
    );
    assert_eq!(
        refuse("{\"color_artifact_reduction\":101}"),
        "colorize color_artifact_reduction must be 0 to 100"
    );
    assert_eq!(
        refuse("{\"color_artifact_reduction\":-1}"),
        "colorize color_artifact_reduction must be 0 to 100"
    );
    // Buffer lengths against side.
    assert_eq!(
        input8(&image, &srgb, side, 3 * cells - 1).unwrap_err(),
        "the colorize input must hold 3 × side² samples"
    );
    assert_eq!(
        apply8(&image, &srgb, side, &zero[1..], "{}").unwrap_err(),
        "the colorize chroma must hold 2 × side² samples"
    );
    assert_eq!(
        input8(&image, &srgb, 8, 3 * 64).unwrap_err(),
        "colorize side must be 16 to 1024"
    );
    // Profiles: none, garbage, CMYK.
    let profile_refusal =
        "colorize needs an RGB matrix/TRC document profile; convert or assign a supported profile first";
    for profile in [Vec::new(), vec![7u8; 300], press_profile("press")] {
        assert_eq!(
            input8(&image, &profile, side, 3 * cells).unwrap_err(),
            profile_refusal
        );
        assert_eq!(
            apply8(&image, &profile, side, &zero, "{}").unwrap_err(),
            profile_refusal
        );
    }
    // A float layer refuses by name at both calls.
    let float =
        PrecisionImage::from_rgba32(2, 2, vec![[0.5; 4]; 4], IccProfile::srgb(), None).unwrap();
    let float_refusal = "neural filters are not available in a 32-bit document";
    assert_eq!(input16(&float, &srgb, side).unwrap_err(), float_refusal);
    assert_eq!(
        apply16(&float, &srgb, side, &zero, "{}").unwrap_err(),
        float_refusal
    );
    // The working limit and a cancel.
    let big = byte(64, 64, &[100u8, 50, 25, 255].repeat(64 * 64));
    let limited = apply8_with(&big, &srgb, side, &zero, "{}", 4096, ptr::null()).unwrap_err();
    assert!(
        limited.starts_with("precision operation needs "),
        "{limited}"
    );
    unsafe {
        let control = dr_job_control_create();
        dr_job_control_cancel(control);
        let cancelled = apply8_with(&big, &srgb, side, &swirl(16, 20.0), "{}", LIMIT, control);
        assert_eq!(cancelled.unwrap_err(), "operation cancelled");
        dr_job_control_free(control);
    }
}

#[test]
fn plan_is_the_larger_phase_plus_bookkeeping() {
    let plan = |w, h, side, bps| {
        let mut err: *mut c_char = ptr::null_mut();
        let bytes = unsafe { dr_neural_colorize_plan(w, h, side, bps, &mut err) };
        if bytes == 0 {
            Err(take_err_string(err))
        } else {
            assert!(err.is_null());
            Ok(bytes)
        }
    };
    let mib = 1u64 << 20;
    let (w, h, side) = (6000u64, 4000u64, 512u64);
    let n = w * h;
    let cells = side * side;
    let input8 = w * side * 4 + cells * 4;
    let apply8 = 2 * cells * 16 + n * 4;
    assert_eq!(plan(6000, 4000, 512, 1).unwrap(), input8.max(apply8) + mib);
    let input16 = w * side * 4 + cells * 4 + n * 8;
    let apply16 = 2 * cells * 16 + n * 8 + 2 * n * 8 + PrecisionToneEncoder::scratch_bytes();
    assert_eq!(
        plan(6000, 4000, 512, 2).unwrap(),
        input16.max(apply16) + mib
    );
    // A thin, wide layer: the input phase's intermediate dominates.
    let (w, h) = (100_000u64, 1u64);
    let input = w * 1024 * 4 + 1024 * 1024 * 4;
    assert_eq!(
        plan(100_000, 1, 1024, 1).unwrap(),
        input.max(2 * 1024 * 1024 * 16 + w * h * 4) + mib
    );
    assert_eq!(
        plan(10, 10, 15, 1).unwrap_err(),
        "colorize side must be 16 to 1024"
    );
    assert_eq!(
        plan(10, 10, 1025, 1).unwrap_err(),
        "colorize side must be 16 to 1024"
    );
    assert_eq!(
        plan(10, 10, 512, 4).unwrap_err(),
        "bytes_per_sample must be 1 or 2"
    );
    assert_eq!(
        plan(0, 10, 512, 1).unwrap_err(),
        "colorize needs a non-empty image"
    );
}
