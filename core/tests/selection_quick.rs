//! Quick Selection's brush, pre-smoothing and hole fill through
//! `dr_selection_quick`. Every expectation is structural and stated from the
//! fixture's own geometry: which side of an edge a pixel is on, whether an
//! island is enclosed, how many brush pixels a call asks for. The one numeric
//! oracle (the former growth) is a naive f64 breadth-first walk written here.

use std::collections::VecDeque;
use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_jobs::{
    dr_job_control_create, dr_job_control_free, dr_job_control_set_scratch_limit,
};
use doctored_core::{ffi::*, ffi_doc::*, ffi_selection_tools::*};
use image::{Rgba, RgbaImage};

use crate::common::*;

const RED: [u8; 4] = [200, 60, 40, 255];
const BLUE: [u8; 4] = [40, 90, 200, 255];
const GREEN: [u8; 4] = [30, 200, 60, 255];

/// `dr_selection_quick` over `image` as both the document and the source. On
/// failure the caller's buffer must be untouched.
fn quick(image: &RgbaImage, params: &str) -> Result<Vec<u8>, String> {
    let (w, h) = image.dimensions();
    let params = CString::new(params).unwrap();
    unsafe {
        let source = dr_image_from_rgba(image.as_raw().as_ptr(), w, h);
        let doc = dr_doc_from_image(source);
        let mut out = vec![77_u8; (w * h) as usize];
        let mut err = ptr::null_mut();
        let ok = dr_selection_quick(
            doc,
            source,
            params.as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            &mut err,
        );
        dr_doc_free(doc);
        dr_image_free(source);
        if ok {
            Ok(out)
        } else {
            assert!(out.iter().all(|v| *v == 77), "a failed call wrote output");
            Err(take_err_string(err))
        }
    }
}

/// A plus of five pixels centred on `(cx, cy)`.
fn plus(cx: u32, cy: u32) -> [(u32, u32); 5] {
    [
        (cx, cy),
        (cx - 1, cy),
        (cx + 1, cy),
        (cx, cy - 1),
        (cx, cy + 1),
    ]
}

fn at(out: &[u8], w: u32, x: u32, y: u32) -> u8 {
    out[(y * w + x) as usize]
}

#[test]
fn a_brush_on_a_noisy_blurred_edge_selects_its_side_solidly_and_stops_in_the_ramp() {
    // Red on the left, blue on the right, a linear ramp over x in 28..36, and
    // ±6 codes of noise on every channel from a fixed LCG.
    let (w, h) = (64_u32, 48_u32);
    let mut state = 0x1234_5678_u32;
    let mut noise = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((state >> 16) % 13) as i32 - 6
    };
    let image = RgbaImage::from_fn(w, h, |x, _| {
        let t = ((f64::from(x) - 28.0) / 8.0).clamp(0.0, 1.0);
        let mut pixel = [0_u8; 4];
        for c in 0..3 {
            let value = f64::from(RED[c]) * (1.0 - t) + f64::from(BLUE[c]) * t;
            pixel[c] = (value.round() as i32 + noise()).clamp(0, 255) as u8;
        }
        pixel[3] = 255;
        Rgba(pixel)
    });
    let out = quick(&image, r#"{"seeds":[[10,24]],"radius":6,"smooth":1}"#).unwrap();
    for y in 0..h {
        for x in 0..w {
            let value = at(&out, w, x, y);
            if x < 28 {
                assert_eq!(value, 255, "the seeded side has a gap at {x},{y}");
            }
            if x >= 36 {
                assert_eq!(value, 0, "the selection passed the ramp at {x},{y}");
            }
        }
    }
}

#[test]
fn a_hard_edge_is_followed_exactly_with_smoothing_on() {
    let image = RgbaImage::from_fn(64, 64, |x, _| Rgba(if x < 32 { RED } else { BLUE }));
    let out = quick(&image, r#"{"seeds":[[16,32]],"radius":6,"smooth":1}"#).unwrap();
    for y in 0..64 {
        for x in 0..64 {
            assert_eq!(at(&out, 64, x, y), if x < 32 { 255 } else { 0 }, "{x},{y}");
        }
    }
}

#[test]
fn an_enclosed_island_is_filled_and_a_large_or_border_island_is_not() {
    // Radius 6 fills at most ceil(pi * 36) = 114 pixels.
    let (w, h) = (80_u32, 60_u32);
    let small = plus(20, 20);
    let large = |x: u32, y: u32| (50..62).contains(&x) && (30..42).contains(&y);
    let border = |x: u32, y: u32| x == 0 && (10..13).contains(&y);
    let image = RgbaImage::from_fn(w, h, |x, y| {
        Rgba(if small.contains(&(x, y)) || large(x, y) || border(x, y) {
            GREEN
        } else {
            RED
        })
    });
    let out = quick(&image, r#"{"seeds":[[40,10]],"radius":6,"smooth":1}"#).unwrap();
    for y in 0..h {
        for x in 0..w {
            let expect = if large(x, y) || border(x, y) { 0 } else { 255 };
            assert_eq!(at(&out, w, x, y), expect, "{x},{y}");
        }
    }
    // The same call with no brush leaves the small island out: the fill is
    // the brush's, not the growth's.
    let plain = quick(&image, r#"{"seeds":[[40,10]]}"#).unwrap();
    for (x, y) in small {
        assert_eq!(at(&plain, w, x, y), 0, "{x},{y}");
    }
}

#[test]
fn transparency_is_a_barrier_and_never_the_wall_of_a_hole() {
    // Two opaque islands on a transparent canvas, 40 px apart.
    let (w, h) = (80_u32, 40_u32);
    let (near, far) = (plus(20, 20), plus(60, 20));
    let image = RgbaImage::from_fn(w, h, |x, y| {
        Rgba(if near.contains(&(x, y)) || far.contains(&(x, y)) {
            RED
        } else {
            [9, 9, 9, 0]
        })
    });
    let out = quick(&image, r#"{"seeds":[[20,20]],"radius":14.5}"#).unwrap();
    for y in 0..h {
        for x in 0..w {
            let expect = if near.contains(&(x, y)) { 255 } else { 0 };
            assert_eq!(at(&out, w, x, y), expect, "{x},{y}");
        }
    }
    // A pocket of another colour inside the selected region, with one
    // transparent pixel beside it: every other neighbour is selected, and it
    // still is not a hole.
    let pocket = plus(30, 20);
    let image = RgbaImage::from_fn(60, 40, |x, y| {
        Rgba(if (x, y) == (32, 20) {
            [0, 0, 0, 0]
        } else if pocket.contains(&(x, y)) {
            GREEN
        } else {
            RED
        })
    });
    let out = quick(&image, r#"{"seeds":[[10,10]],"radius":6}"#).unwrap();
    for y in 0..40 {
        for x in 0..60 {
            let expect = if pocket.contains(&(x, y)) || (x, y) == (32, 20) {
                0
            } else {
                255
            };
            assert_eq!(at(&out, 60, x, y), expect, "{x},{y}");
        }
    }
}

#[test]
fn the_brush_pixel_cap_names_the_brush_size_and_the_seed_count() {
    // A radius-149.5 brush at the centre of 320x320 visits a 299x299 box:
    // 700 seeds are 62,580,700 box pixels, 800 are 71,520,800.
    let image = RgbaImage::from_pixel(320, 320, Rgba(RED));
    let request = |count: usize| {
        let seeds = vec!["[160,160]"; count].join(",");
        format!(r#"{{"seeds":[{seeds}],"radius":149.5}}"#)
    };
    let out = quick(&image, &request(700)).unwrap();
    assert!(out.iter().all(|v| *v == 255));
    let message = quick(&image, &request(800)).unwrap_err();
    assert!(message.contains("brush size 300"), "{message}");
    assert!(message.contains("800 seeds"), "{message}");
}

#[test]
fn bad_brush_parameters_refuse() {
    let image = RgbaImage::from_pixel(8, 8, Rgba(RED));
    for request in [
        r#"{"seeds":[[1,1]],"radius":-1}"#,
        r#"{"seeds":[[1,1]],"radius":257}"#,
        r#"{"seeds":[[1,1]],"radius":"6"}"#,
        r#"{"seeds":[[1,1]],"smooth":5}"#,
        r#"{"seeds":[[1,1]],"smooth":1.5}"#,
        r#"{"seeds":[[1,1]],"smooth":-1}"#,
    ] {
        assert!(!quick(&image, request).unwrap_err().is_empty(), "{request}");
    }
}

/// `dr_selection_quick` under a job control whose scratch limit is `limit`.
fn quick_limited(image: &RgbaImage, params: &str, limit: u64) -> Result<Vec<u8>, String> {
    let (w, h) = image.dimensions();
    let params = CString::new(params).unwrap();
    unsafe {
        let source = dr_image_from_rgba(image.as_raw().as_ptr(), w, h);
        let doc = dr_doc_from_image(source);
        let token = dr_job_control_create();
        assert!(dr_job_control_set_scratch_limit(token, limit));
        let mut out = vec![77_u8; (w * h) as usize];
        let mut err = ptr::null_mut();
        let ok = dr_selection_quick(
            doc,
            source,
            params.as_ptr(),
            out.as_mut_ptr(),
            token,
            &mut err,
        );
        dr_job_control_free(token);
        dr_doc_free(doc);
        dr_image_free(source);
        if ok {
            Ok(out)
        } else {
            Err(take_err_string(err))
        }
    }
}

#[test]
fn the_reservation_is_the_larger_of_the_smoothing_and_the_growth_peak() {
    const MIB: u64 = 1024 * 1024;
    let params = r#"{"seeds":[[10,2]],"radius":2,"smooth":4}"#;
    // 70,000 × 5 with smooth 4: the ring is min(4 + 1, 5) = all five rows, a
    // second Lab plane, so pre-smoothing (12n + 12·70,000·5 = 8,400,000) is
    // above growth's 21n (7,350,000) and is the figure reserved.
    let (w, h) = (70_000_u32, 5_u32);
    let n = u64::from(w * h);
    let wide = RgbaImage::from_pixel(w, h, Rgba(RED));
    let peak = 12 * n + 12 * 70_000 * 5 + MIB;
    assert!(peak > 21 * n + MIB);
    let out = quick_limited(&wide, params, peak).unwrap();
    assert!(out.iter().all(|v| *v == 255));
    let message = quick_limited(&wide, params, peak - 1).unwrap_err();
    assert!(message.contains("working memory"), "{message}");
    // On a square canvas the ring is five rows of 64 and growth's 21n is the
    // peak, as before.
    let square = RgbaImage::from_pixel(64, 64, Rgba(RED));
    let out = quick_limited(&square, params, 21 * 64 * 64 + MIB).unwrap();
    assert!(out.iter().all(|v| *v == 255));
    let message = quick_limited(&square, params, 21 * 64 * 64 + MIB - 1).unwrap_err();
    assert!(message.contains("working memory"), "{message}");
}

// ---------------------------------------------- the former growth, in f64 --

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// CIE L* of a neutral sRGB grey (a* and b* are 0 for a neutral).
fn lightness(grey: u8) -> f64 {
    let y = srgb_decode(f64::from(grey) / 255.0);
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        24389.0 / 27.0 * y
    }
}

/// Seeded four-connected growth over a grey image: a neighbour joins when its
/// step from the claiming pixel is at most `edge` and its distance from that
/// pixel's seed is at most `tolerance`. Panics when any comparison lies within
/// 0.01 of its threshold, so the f32 kernel cannot legitimately disagree.
fn grey_growth(
    grey: &[u8],
    (w, h): (usize, usize),
    seeds: &[(usize, usize)],
    tolerance: f64,
    edge: f64,
) -> Vec<u8> {
    let mut out = vec![0_u8; w * h];
    let mut queue = VecDeque::new();
    for &(x, y) in seeds {
        if out[y * w + x] == 0 {
            out[y * w + x] = 255;
            queue.push_back((y * w + x, y * w + x));
        }
    }
    let within = |a: usize, b: usize, limit: f64| {
        let distance = (lightness(grey[a]) - lightness(grey[b])).abs();
        assert!(
            (distance - limit).abs() > 0.01,
            "borderline comparison in the fixture"
        );
        distance <= limit
    };
    while let Some((index, anchor)) = queue.pop_front() {
        let (x, y) = (index % w, index / w);
        let mut visit = |nx: usize, ny: usize| {
            let next = ny * w + nx;
            if out[next] == 0 && within(next, index, edge) && within(next, anchor, tolerance) {
                out[next] = 255;
                queue.push_back((next, anchor));
            }
        };
        if x > 0 {
            visit(x - 1, y);
        }
        if x + 1 < w {
            visit(x + 1, y);
        }
        if y > 0 {
            visit(x, y - 1);
        }
        if y + 1 < h {
            visit(x, y + 1);
        }
    }
    out
}

#[test]
fn radius_zero_and_smooth_zero_are_the_plain_seeded_growth() {
    // `selection_tools.rs`'s gradient fixture: a ramp of 8-code steps, then a
    // jump to 150.
    let grey: Vec<u8> = (0..18_u32)
        .map(|i| {
            let x = i % 9;
            if x < 6 {
                20 + x as u8 * 8
            } else {
                150
            }
        })
        .collect();
    let image = RgbaImage::from_fn(9, 2, |x, y| {
        let v = grey[(y * 9 + x) as usize];
        Rgba([v, v, v, 255])
    });
    for (seeds, list, tolerance, edge) in [
        ("[[0,0]]", vec![(0, 0)], 12.0, 8.0),
        ("[[0,0],[8,1]]", vec![(0, 0), (8, 1)], 200.0, 8.0),
        ("[[0,0]]", vec![(0, 0)], 200.0, 8.0),
    ] {
        let expect = grey_growth(&grey, (9, 2), &list, tolerance, edge);
        let plain = format!(r#"{{"seeds":{seeds},"tolerance":{tolerance},"edge":{edge}}}"#);
        let explicit = format!(
            r#"{{"seeds":{seeds},"tolerance":{tolerance},"edge":{edge},"radius":0,"smooth":0}}"#
        );
        assert_eq!(quick(&image, &plain).unwrap(), expect, "{plain}");
        assert_eq!(quick(&image, &explicit).unwrap(), expect, "{explicit}");
    }
}
