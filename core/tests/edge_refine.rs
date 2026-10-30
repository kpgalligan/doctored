//! Select and Mask's edge stage at 8 bits, through
//! `dr_selection_refine`. The fixtures are composites of two known colours
//! under a known alpha, so the refined coverage is measured against that
//! alpha; the kernel's arithmetic is checked against a naive windowed f64
//! regression written here. Bounds were fixed before the kernel was tuned.

use std::ffi::CString;
use std::ptr;

use doctored_core::{ffi::*, ffi_jobs::*, ffi_selection_tools::*};

use crate::common::*;

/// A `size`² composite `C = round(a·F + (1 − a)·B)` whose alpha is a disc of
/// `radius` about the centre with a 6 px linear ramp (0.5 at the radius).
struct Composite {
    size: u32,
    bytes: Vec<u8>,
    /// The true alpha, 0…1.
    alpha: Vec<f64>,
    /// Each pixel centre's distance from the alpha's 0.5 contour.
    from_contour: Vec<f64>,
    /// The hard input selection: alpha ≥ 0.5.
    mask: Vec<u8>,
}

fn composite(size: u32, radius: f64, f: [u8; 3], b: [u8; 3]) -> Composite {
    let centre = f64::from(size) / 2.0;
    let (mut bytes, mut alpha, mut from_contour, mut mask) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for y in 0..size {
        for x in 0..size {
            let d = (f64::from(x) + 0.5 - centre).hypot(f64::from(y) + 0.5 - centre);
            let a = ((radius + 3.0 - d) / 6.0).clamp(0.0, 1.0);
            for c in 0..3 {
                bytes.push((a * f64::from(f[c]) + (1.0 - a) * f64::from(b[c])).round() as u8);
            }
            bytes.push(255);
            alpha.push(a);
            from_contour.push((d - radius).abs());
            mask.push(if a >= 0.5 { 255 } else { 0 });
        }
    }
    Composite {
        size,
        bytes,
        alpha,
        from_contour,
        mask,
    }
}

/// `dr_selection_refine` under an optional scratch limit. On failure the
/// caller's buffer must be untouched and the control's reservation released.
fn refine_limited(
    rgba: &[u8],
    (w, h): (u32, u32),
    mask: &[u8],
    area: Option<&[u8]>,
    params: &str,
    limit: Option<u64>,
) -> Result<Vec<u8>, String> {
    let params = CString::new(params).unwrap();
    unsafe {
        let source = dr_image_from_rgba(rgba.as_ptr(), w, h);
        let token = limit.map(|bytes| {
            let token = dr_job_control_create();
            assert!(dr_job_control_set_scratch_limit(token, bytes));
            token
        });
        let mut out = vec![77_u8; mask.len()];
        let mut err = ptr::null_mut();
        let ok = dr_selection_refine(
            source,
            mask.as_ptr(),
            area.map_or(ptr::null(), <[u8]>::as_ptr),
            params.as_ptr(),
            out.as_mut_ptr(),
            token.map_or(ptr::null(), |t| t.cast_const()),
            &mut err,
        );
        if let Some(token) = token {
            assert_eq!(dr_job_control_scratch_bytes(token), 0);
            dr_job_control_free(token);
        }
        dr_image_free(source);
        if ok {
            Ok(out)
        } else {
            assert!(out.iter().all(|v| *v == 77), "a failed call wrote output");
            Err(take_err_string(err))
        }
    }
}

fn refine(
    rgba: &[u8],
    size: (u32, u32),
    mask: &[u8],
    area: Option<&[u8]>,
    params: &str,
) -> Result<Vec<u8>, String> {
    refine_limited(rgba, size, mask, area, params, None)
}

/// The known-alpha bounds: the mean error over fractional-alpha pixels, the
/// largest error anywhere, and identity beyond 9 px of the contour.
fn assert_recovers_alpha(k: &Composite, out: &[u8], mean_bound: f64, what: &str) {
    let (mut total, mut count, mut worst) = (0.0, 0, 0.0_f64);
    for (i, &value) in out.iter().enumerate() {
        let error = (f64::from(value) / 255.0 - k.alpha[i]).abs();
        worst = worst.max(error);
        if k.alpha[i] > 0.0 && k.alpha[i] < 1.0 {
            total += error;
            count += 1;
        }
        if k.from_contour[i] > 9.0 {
            assert_eq!(value, k.mask[i], "{what}: pixel {i} is outside the band");
        }
    }
    let mean = total / f64::from(count);
    assert!(
        mean <= mean_bound / 255.0,
        "{what}: mean error {}",
        mean * 255.0
    );
    assert!(
        worst <= 64.0 / 255.0,
        "{what}: worst error {}",
        worst * 255.0
    );
    assert_ne!(out, &k.mask[..], "{what}: nothing was refined");
}

#[test]
fn a_hard_selection_recovers_the_known_alpha_of_a_two_colour_composite() {
    let k = composite(96, 30.0, [200, 60, 40], [40, 90, 200]);
    let size = (k.size, k.size);
    let plain = refine(&k.bytes, size, &k.mask, None, r#"{"edge_radius":8}"#).unwrap();
    assert_recovers_alpha(&k, &plain, 16.0, "smart off");
    let smart = refine(
        &k.bytes,
        size,
        &k.mask,
        None,
        r#"{"edge_radius":8,"smart_radius":true}"#,
    )
    .unwrap();
    assert_recovers_alpha(&k, &smart, 24.0, "smart on");
}

#[test]
fn two_colours_of_equal_luminance_are_separated_by_colour() {
    // Rec. 709 luma of both is 81.9: a luma guide sees no edge here.
    let k = composite(96, 30.0, [200, 50, 50], [50, 92, 76]);
    let size = (k.size, k.size);
    let plain = refine(&k.bytes, size, &k.mask, None, r#"{"edge_radius":8}"#).unwrap();
    assert_recovers_alpha(&k, &plain, 16.0, "smart off");
    let smart = refine(
        &k.bytes,
        size,
        &k.mask,
        None,
        r#"{"edge_radius":8,"smart_radius":true}"#,
    )
    .unwrap();
    assert_recovers_alpha(&k, &smart, 24.0, "smart on");
}

#[test]
fn smart_radius_keeps_a_hard_edge_across_flat_colour() {
    let (w, h) = (64_u32, 64_u32);
    let rgba: Vec<u8> = (0..w * h).flat_map(|_| [120, 120, 120, 255]).collect();
    let mask: Vec<u8> = (0..w * h)
        .map(|i| if i % w < 32 { 255 } else { 0 })
        .collect();
    let change = |out: &[u8]| {
        out.iter()
            .zip(&mask)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap()
    };
    let plain = refine(&rgba, (w, h), &mask, None, r#"{"edge_radius":8}"#).unwrap();
    assert!(change(&plain) >= 32, "smart off moved {}", change(&plain));
    let smart = refine(
        &rgba,
        (w, h),
        &mask,
        None,
        r#"{"edge_radius":8,"smart_radius":true}"#,
    )
    .unwrap();
    assert!(change(&smart) <= 2, "smart on moved {}", change(&smart));
}

#[test]
fn the_area_plane_excludes_includes_and_refuses_other_values() {
    let k = composite(96, 30.0, [200, 60, 40], [40, 90, 200]);
    let size = (k.size, k.size);
    let n = k.mask.len();
    let column = |i: usize| i % 96;
    // EXCLUDE on the right half: those pixels keep the input exactly, and the
    // left half is still refined.
    let area: Vec<u8> = (0..n)
        .map(|i| if column(i) >= 48 { 0 } else { 128 })
        .collect();
    let out = refine(&k.bytes, size, &k.mask, Some(&area), r#"{"edge_radius":8}"#).unwrap();
    let mut refined = 0;
    for (i, &value) in out.iter().enumerate() {
        if column(i) >= 48 {
            assert_eq!(value, k.mask[i], "excluded pixel {i}");
        } else if value != k.mask[i] {
            refined += 1;
        }
    }
    assert!(refined > 0);
    // INCLUDE with no radius: refinement happens under the stroke only.
    let area: Vec<u8> = (0..n)
        .map(|i| if column(i) < 30 { 255 } else { 128 })
        .collect();
    let out = refine(&k.bytes, size, &k.mask, Some(&area), "{}").unwrap();
    let mut refined = 0;
    for (i, &value) in out.iter().enumerate() {
        if column(i) >= 30 {
            assert_eq!(value, k.mask[i], "pixel {i} is outside the stroke");
        } else if value != k.mask[i] {
            refined += 1;
        }
    }
    assert!(refined > 0);
    // No radius and no INCLUDE: the input bytes, with or without a plane.
    let automatic = vec![128_u8; n];
    for area in [None, Some(&automatic[..])] {
        assert_eq!(
            refine(&k.bytes, size, &k.mask, area, r#"{"edge_radius":0}"#).unwrap(),
            k.mask
        );
    }
    // Any other value is an error, whatever the radius.
    let mut bad = automatic.clone();
    bad[100] = 7;
    for params in ["{}", r#"{"edge_radius":8}"#] {
        let message = refine(&k.bytes, size, &k.mask, Some(&bad), params).unwrap_err();
        assert!(message.contains("0 (exclude), 128 (automatic) or 255 (include)"));
    }
}

#[test]
fn the_edge_stage_reserves_its_own_budget_in_both_of_its_cases() {
    let k = composite(200, 60.0, [200, 60, 40], [40, 90, 200]);
    let size = (k.size, k.size);
    let n = 200 * 200_u64;
    let line = 20 * 200 + 1024 * 1024;
    // An INCLUDE stroke over a stretch of the contour.
    let stroke: Vec<u8> = (0..n as usize)
        .map(|i| if i % 200 < 50 { 255 } else { 128 })
        .collect();
    let cases: [(&str, Option<&[u8]>); 2] =
        [(r#"{"edge_radius":8}"#, None), ("{}", Some(&stroke[..]))];
    for (params, area) in cases {
        // What the host grants for the edge stage (66n less its own output).
        let out = refine_limited(&k.bytes, size, &k.mask, area, params, Some(65 * n + line))
            .unwrap_or_else(|message| panic!("{params}: {message}"));
        assert_ne!(out, k.mask, "{params}");
        // The grant of a refinement with no edge stage is not enough.
        let message =
            refine_limited(&k.bytes, size, &k.mask, area, params, Some(9 * n + line)).unwrap_err();
        assert!(message.contains("working memory"), "{params}: {message}");
    }
    // The four stages alone still fit the old grant.
    refine_limited(
        &k.bytes,
        size,
        &k.mask,
        None,
        r#"{"shift":2,"feather":1}"#,
        Some(9 * n + line),
    )
    .unwrap();
}

#[test]
fn a_cancelled_edge_refinement_publishes_nothing() {
    let k = composite(96, 30.0, [200, 60, 40], [40, 90, 200]);
    let params = CString::new(r#"{"edge_radius":8}"#).unwrap();
    unsafe {
        let source = dr_image_from_rgba(k.bytes.as_ptr(), 96, 96);
        let token = dr_job_control_create();
        dr_job_control_cancel(token);
        let mut out = vec![77_u8; k.mask.len()];
        let mut err = ptr::null_mut();
        assert!(!dr_selection_refine(
            source,
            k.mask.as_ptr(),
            ptr::null(),
            params.as_ptr(),
            out.as_mut_ptr(),
            token,
            &mut err,
        ));
        assert!(take_err_string(err).contains("cancelled"));
        assert!(out.iter().all(|v| *v == 77));
        assert_eq!(dr_job_control_scratch_bytes(token), 0);
        dr_job_control_free(token);
        dr_image_free(source);
    }
}

// ------------------------------------- the kernel's arithmetic, naively --

/// The documented estimate at every pixel of a `w`×`h` image, as direct
/// O(N·r²) window sums in f64 with no running sums: per channel the guided
/// filter's `a`, `b` and confidence over each clipped window, then `q` from
/// the window means of `a` and `b`, combined by confidence.
fn naive_estimate(
    rgba: &[u8],
    mask: &[u8],
    (w, h): (usize, usize),
    r: usize,
    smart: bool,
) -> Vec<f64> {
    let window = |x: usize, y: usize| {
        (y.saturating_sub(r)..=(y + r).min(h - 1))
            .flat_map(move |ny| {
                (x.saturating_sub(r)..=(x + r).min(w - 1)).map(move |nx| ny * w + nx)
            })
            .collect::<Vec<_>>()
    };
    let p: Vec<f64> = mask.iter().map(|v| f64::from(*v) / 255.0).collect();
    let n = w * h;
    let (mut weighted, mut weight, mut best) = (vec![0.0; n], vec![0.0; n], vec![0.0_f64; n]);
    for c in 0..3 {
        let guide: Vec<f64> = (0..n)
            .map(|i| f64::from(rgba[i * 4 + c]) / 255.0 * (f64::from(rgba[i * 4 + 3]) / 255.0))
            .collect();
        let (mut a, mut b, mut k) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
        for y in 0..h {
            for x in 0..w {
                let cells = window(x, y);
                let count = cells.len() as f64;
                let mean =
                    |f: &dyn Fn(usize) -> f64| cells.iter().map(|&i| f(i)).sum::<f64>() / count;
                let (mi, mp) = (mean(&|i| guide[i]), mean(&|i| p[i]));
                let var_i = mean(&|i| (guide[i] - mi).powi(2));
                let var_p = mean(&|i| (p[i] - mp).powi(2));
                let cov = mean(&|i| (guide[i] - mi) * (p[i] - mp));
                let i = y * w + x;
                a[i] = cov / (var_i + 0.0004);
                b[i] = mp - a[i] * mi;
                k[i] = (cov * cov / ((var_i + 0.0004) * (var_p + 1e-6))).clamp(0.0, 1.0);
            }
        }
        for y in 0..h {
            for x in 0..w {
                let cells = window(x, y);
                let count = cells.len() as f64;
                let ma = cells.iter().map(|&i| a[i]).sum::<f64>() / count;
                let mb = cells.iter().map(|&i| b[i]).sum::<f64>() / count;
                let i = y * w + x;
                let q = (ma * guide[i] + mb).clamp(0.0, 1.0);
                let share = k[i] * k[i] + 1e-4;
                weighted[i] += share * q;
                weight[i] += share;
                best[i] = best[i].max(k[i]);
            }
        }
    }
    (0..n)
        .map(|i| {
            let q = weighted[i] / weight[i];
            if smart {
                p[i] + best[i] * (q - p[i])
            } else {
                q
            }
        })
        .collect()
}

#[test]
fn the_estimate_matches_a_direct_windowed_regression() {
    // Every pixel INCLUDE and no radius: the whole plane is written, with the
    // minimum window half-width of 4.
    let (w, h) = (13_usize, 11_usize);
    let n = w * h;
    let rgba: Vec<u8> = (0..n)
        .flat_map(|i| {
            [
                ((i * 47 + 23) % 256) as u8,
                ((i * 89 + 5) % 256) as u8,
                ((i * 151 + 77) % 256) as u8,
                if i % 7 == 3 { 180 } else { 255 },
            ]
        })
        .collect();
    let mask: Vec<u8> = (0..n).map(|i| ((i * 31 + 7) % 256) as u8).collect();
    let area = vec![255_u8; n];
    for smart in [false, true] {
        let params = format!(r#"{{"smart_radius":{smart}}}"#);
        let out = refine(&rgba, (w as u32, h as u32), &mask, Some(&area), &params).unwrap();
        let expect = naive_estimate(&rgba, &mask, (w, h), 4, smart);
        for i in 0..n {
            let reference = (expect[i].clamp(0.0, 1.0) * 255.0).round() as u8;
            assert!(
                out[i].abs_diff(reference) <= 1,
                "smart {smart}, pixel {i}: {} != {reference}",
                out[i]
            );
        }
    }
}

#[test]
fn the_band_is_the_signed_distance_to_the_half_coverage_contour() {
    // A hard vertical edge between columns 19 and 20 across a colour edge at
    // the same place. With radius 3 the band is the pixels whose centre is
    // within 3.5 px of the nearest pixel centre of the other side: columns
    // 17..=22.
    let (w, h) = (40_u32, 24_u32);
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let t = ((f64::from(i % w) - 15.5) / 8.0).clamp(0.0, 1.0);
            [
                (200.0 - 160.0 * t).round() as u8,
                (60.0 + 30.0 * t).round() as u8,
                (40.0 + 160.0 * t).round() as u8,
                255,
            ]
        })
        .collect();
    let mask: Vec<u8> = (0..w * h)
        .map(|i| if i % w < 20 { 255 } else { 0 })
        .collect();
    let out = refine(&rgba, (w, h), &mask, None, r#"{"edge_radius":3}"#).unwrap();
    let mut refined = [false; 40];
    for (i, (a, b)) in out.iter().zip(&mask).enumerate() {
        let column = i % w as usize;
        if !(17..=22).contains(&column) {
            assert_eq!(a, b, "column {column} is outside the band");
        } else if a != b {
            refined[column] = true;
        }
    }
    assert!(refined[19] && refined[20]);
}
