//! Independent geometric-area and source-over references for native artwork.
//! Polygon coverage uses pixel clipping + shoelace area, not the production
//! scanline integration. Curves have analytic graph integrals or a converged
//! uniform-parametric reference; no production rasterizer builds expected data.
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::*;
use doctored_core::ffi_doc::dr_doc_layer_meta;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::json;
use std::collections::HashSet;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};

type Point = [f64; 2];
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
// Flattening is specified within 1/4096 pixel. This declared per-pixel area
// gate allows two intersecting boundaries' displacement, not a byte-level grid.
const CURVE_BOUND: u16 = 32;

unsafe fn take_text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(pointer) };
    value
}

#[allow(clippy::too_many_arguments)]
fn coverage_request(
    commands: &[u8],
    points: &[Point],
    width: u32,
    height: u32,
    even_odd: bool,
    output: &mut [u16],
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<(), String> {
    let mut error = ptr::null_mut();
    let accepted = unsafe {
        dr_artwork_path_coverage(
            commands.as_ptr(),
            commands.len(),
            points.as_ptr().cast(),
            points.len(),
            width,
            height,
            even_odd,
            output.as_mut_ptr(),
            output.len(),
            limit,
            other,
            control.map_or(ptr::null(), |value| value),
            &mut error,
        )
    };
    let message = unsafe { take_text(error) };
    if accepted {
        assert!(message.is_empty(), "successful coverage reported {message}");
        Ok(())
    } else {
        assert!(!message.is_empty(), "coverage refusal lacks diagnostic");
        Err(message)
    }
}
fn coverage(
    commands: &[u8],
    points: &[Point],
    width: u32,
    height: u32,
    even_odd: bool,
) -> Vec<u16> {
    let mut result = vec![0x4a4a; (width * height) as usize];
    coverage_request(
        commands,
        points,
        width,
        height,
        even_odd,
        &mut result,
        LIMIT,
        0,
        None,
    )
    .unwrap();
    result
}
fn polygon_commands(points: &[Point]) -> Vec<u8> {
    let mut result = vec![1; points.len() + 1];
    result[0] = 0;
    *result.last_mut().unwrap() = 4;
    result
}
fn rectangle(left: f64, top: f64, right: f64, bottom: f64) -> Vec<Point> {
    vec![[left, top], [right, top], [right, bottom], [left, bottom]]
}
fn area_code(area: f64) -> u16 {
    (area.clamp(0.0, 1.0) * 65535.0).round() as u16
}

// Sutherland-Hodgman clipping against the four sides of a unit pixel. The
// resulting polygon's signed shoelace integral is independent of scan rows.
fn clipped_area(polygon: &[Point], x: u32, y: u32) -> f64 {
    let mut polygon = polygon.to_vec();
    for (axis, boundary, lower) in [
        (0, f64::from(x), true),
        (0, f64::from(x) + 1.0, false),
        (1, f64::from(y), true),
        (1, f64::from(y) + 1.0, false),
    ] {
        let input = std::mem::take(&mut polygon);
        if input.is_empty() {
            return 0.0;
        }
        let inside = |p: Point| {
            if lower {
                p[axis] >= boundary
            } else {
                p[axis] <= boundary
            }
        };
        let mut previous = *input.last().unwrap();
        for current in input {
            if inside(current) != inside(previous) {
                let t = (boundary - previous[axis]) / (current[axis] - previous[axis]);
                polygon.push([
                    previous[0] + t * (current[0] - previous[0]),
                    previous[1] + t * (current[1] - previous[1]),
                ]);
            }
            if inside(current) {
                polygon.push(current);
            }
            previous = current;
        }
    }
    if polygon.is_empty() {
        return 0.0;
    }
    let mut twice = 0.0;
    for (a, b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        twice += a[0] * b[1] - a[1] * b[0];
    }
    twice.abs() * 0.5
}
fn assert_polygon(points: &[Point], width: u32, height: u32) {
    let result = coverage(&polygon_commands(points), points, width, height, false);
    for y in 0..height {
        for x in 0..width {
            let expected = area_code(clipped_area(points, x, y));
            assert!(
                result[(y * width + x) as usize].abs_diff(expected) <= 1,
                "pixel({x},{y}) actual{} expected{expected}",
                result[(y * width + x) as usize]
            );
        }
    }
}

#[test]
fn fractional_x_and_y_rectangles_match_product_of_exact_interval_intersections() {
    for bounds in [
        [0.173, 0.319, 3.741, 2.827],
        [-0.25, -0.75, 2.125, 1.1875],
        [0.49997, 0.50003, 0.99998, 0.99999],
        [0.1, 0.300001, 3.9, 0.300099],
    ] {
        let points = rectangle(bounds[0], bounds[1], bounds[2], bounds[3]);
        let result = coverage(&polygon_commands(&points), &points, 5, 4, false);
        for y in 0..4 {
            for x in 0..5 {
                let dx = (bounds[2].min(f64::from(x) + 1.0) - bounds[0].max(f64::from(x))).max(0.0);
                let dy = (bounds[3].min(f64::from(y) + 1.0) - bounds[1].max(f64::from(y))).max(0.0);
                assert_eq!(result[(y * 5 + x) as usize], area_code(dx * dy));
            }
        }
    }
}

#[test]
fn slanted_polygon_pixels_match_independent_clipping_area_including_off_canvas() {
    for points in [
        vec![[-1.7, 0.37], [5.4, -0.8], [4.23, 4.8], [0.18, 3.91]],
        vec![[0.07, 0.13], [5.31, 1.37], [2.83, 4.61]],
        vec![
            [0.12, 0.18],
            [5.2, 0.53],
            [3.63, 1.47],
            [4.31, 3.72],
            [1.11, 4.3],
            [0.87, 2.14],
        ],
    ] {
        assert_polygon(&points, 6, 5);
        let reversed: Vec<_> = points.iter().rev().copied().collect();
        assert_eq!(
            coverage(&polygon_commands(&points), &points, 6, 5, false),
            coverage(&polygon_commands(&reversed), &reversed, 6, 5, false)
        );
    }
}

#[test]
fn thin_slanted_edge_produces_more_than_256_native_coverage_values() {
    let points = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1024.0]];
    let result = coverage(&polygon_commands(&points), &points, 1, 1024, false);
    let unique: HashSet<_> = result.iter().copied().collect();
    assert_eq!(unique.len(), 1024);
    assert!(result.iter().filter(|value| **value % 257 != 0).count() > 1000);
    for (y, actual) in result.iter().enumerate() {
        // Integral of 1-y/1024 over this unit row.
        let expected = area_code(1.0 - (y as f64 + 0.5) / 1024.0);
        assert_eq!(*actual, expected);
    }
}

#[test]
fn holes_distinguish_even_odd_and_nonzero_winding_without_cancellation_of_coverage() {
    let outside = rectangle(0.17, 0.29, 5.83, 4.61);
    let inside = rectangle(1.13, 1.77, 4.41, 3.26);
    for reversed in [false, true] {
        let mut points = outside.clone();
        points.extend(if reversed {
            inside.iter().rev().copied().collect::<Vec<_>>()
        } else {
            inside.clone()
        });
        let commands = [polygon_commands(&outside), polygon_commands(&inside)].concat();
        for even_odd in [false, true] {
            let result = coverage(&commands, &points, 6, 5, even_odd);
            for y in 0..5 {
                for x in 0..6 {
                    let hole = if even_odd || reversed {
                        clipped_area(&inside, x, y)
                    } else {
                        0.0
                    };
                    let expected = area_code(clipped_area(&outside, x, y) - hole);
                    assert!(result[(y * 6 + x) as usize].abs_diff(expected) <= 1);
                }
            }
        }
    }
}

#[test]
fn self_intersection_is_union_of_two_independent_triangles() {
    let points = vec![
        [0.125, 0.125],
        [4.875, 4.875],
        [0.125, 4.875],
        [4.875, 0.125],
    ];
    let top = vec![points[0], points[3], [2.5, 2.5]];
    let bottom = vec![[2.5, 2.5], points[1], points[2]];
    for even_odd in [false, true] {
        let result = coverage(&polygon_commands(&points), &points, 5, 5, even_odd);
        for y in 0..5 {
            for x in 0..5 {
                assert_eq!(
                    result[(y * 5 + x) as usize],
                    area_code(clipped_area(&top, x, y) + clipped_area(&bottom, x, y))
                );
            }
        }
    }
}

fn graph_area(x: u32, y: u32, power: i32) -> f64 {
    let scale = 4.0_f64.powi(power - 1);
    let left = f64::from(x);
    let right = left + 1.0;
    let top = f64::from(y);
    let bottom = top + 1.0;
    let full_until = (top * scale).powf(1.0 / f64::from(power));
    let empty_from = (bottom * scale).powf(1.0 / f64::from(power));
    let full = (right.min(full_until) - left).max(0.0);
    let a = left.max(full_until);
    let b = right.min(empty_from);
    full + if b > a {
        bottom * (b - a) - (b.powi(power + 1) - a.powi(power + 1)) / (scale * f64::from(power + 1))
    } else {
        0.0
    }
}

#[test]
fn quadratic_and_cubic_graphs_match_closed_form_pixel_integrals() {
    let cases = [
        (
            vec![0, 2, 1, 4],
            vec![[0.0, 0.0], [2.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
            2,
        ),
        (
            vec![0, 3, 1, 4],
            vec![
                [0.0, 0.0],
                [4.0 / 3.0, 0.0],
                [8.0 / 3.0, 0.0],
                [4.0, 4.0],
                [0.0, 4.0],
            ],
            3,
        ),
    ];
    for (commands, points, power) in cases {
        let result = coverage(&commands, &points, 4, 4, false);
        for y in 0..4 {
            for x in 0..4 {
                let expected = area_code(graph_area(x, y, power));
                assert!(
                    result[(y * 4 + x) as usize].abs_diff(expected) <= CURVE_BOUND,
                    "power{power} pixel({x},{y}) actual{} expected{expected}",
                    result[(y * 4 + x) as usize]
                );
            }
        }
    }
}

fn bezier(points: [Point; 4], t: f64) -> Point {
    let s = 1.0 - t;
    std::array::from_fn(|axis| {
        s.powi(3) * points[0][axis]
            + 3.0 * s * s * t * points[1][axis]
            + 3.0 * s * t * t * points[2][axis]
            + t.powi(3) * points[3][axis]
    })
}
#[test]
fn closed_cubic_cusp_and_backtracking_match_converged_uniform_parameter_reference() {
    // Start==end makes the chord degenerate; x also reverses twice. Dropping
    // that curve or testing only distance to an infinite chord loses its lobe.
    let curve = [[3.0, 4.0], [6.0, 0.0], [0.0, 0.0], [3.0, 4.0]];
    let coarse: Vec<_> = (0..4096)
        .map(|i| bezier(curve, f64::from(i) / 4096.0))
        .collect();
    let fine: Vec<_> = (0..8192)
        .map(|i| bezier(curve, f64::from(i) / 8192.0))
        .collect();
    let result = coverage(&[0, 3, 4], &curve, 6, 5, false);
    let mut nonempty = 0;
    for y in 0..5 {
        for x in 0..6 {
            let reference = area_code(clipped_area(&fine, x, y));
            assert!(
                reference.abs_diff(area_code(clipped_area(&coarse, x, y))) <= 1,
                "independent curve reference has not converged"
            );
            assert!(result[(y * 6 + x) as usize].abs_diff(reference) <= CURVE_BOUND);
            nonempty += usize::from(result[(y * 6 + x) as usize] != 0);
        }
    }
    assert!(nonempty >= 6, "closed cubic vanished");
}

#[test]
fn open_subpaths_close_implicitly_and_empty_paths_are_transparent() {
    let points = rectangle(0.13, 0.27, 2.81, 2.59);
    let closed = coverage(&polygon_commands(&points), &points, 3, 3, false);
    assert_eq!(closed, coverage(&[0, 1, 1, 1], &points, 3, 3, false));
    assert_eq!(coverage(&[], &[], 3, 2, false), vec![0; 6]);
}

#[test]
fn malformed_paths_and_memory_refusals_leave_the_entire_output_untouched() {
    let valid = rectangle(0.1, 0.2, 2.3, 2.7);
    for (commands, points) in [
        (vec![5], vec![]),
        (vec![1], vec![[0.0, 0.0]]),
        (vec![0, 2], vec![[0.0, 0.0], [1.0, 1.0]]),
        (vec![0], vec![[0.0, 0.0], [1.0, 1.0]]),
        (vec![0], vec![[f64::NAN, 0.0]]),
        (vec![0], vec![[0.0, f64::INFINITY]]),
        (vec![0], vec![[1_000_000_001.0, 0.0]]),
    ] {
        let mut out = [0x4a4a; 9];
        assert!(
            coverage_request(&commands, &points, 3, 3, false, &mut out, LIMIT, 0, None).is_err()
        );
        assert_eq!(out, [0x4a4a; 9]);
    }
    for (limit, other, count) in [
        (1, 0, 9),
        (LIMIT, LIMIT, 9),
        (LIMIT, u64::MAX, 9),
        (LIMIT, 0, 8),
    ] {
        let mut out = vec![0x4a4a; count];
        assert!(coverage_request(
            &polygon_commands(&valid),
            &valid,
            3,
            3,
            false,
            &mut out,
            limit,
            other,
            None
        )
        .is_err());
        assert_eq!(out, vec![0x4a4a; count]);
    }
}

#[test]
fn path_work_and_command_caps_refuse_atomically_without_raising_limits() {
    let points = vec![[0.5, 0.5]; 262_145];
    let commands = vec![0; points.len()];
    let mut out = [0x4a4a; 4];
    assert!(coverage_request(&commands, &points, 2, 2, false, &mut out, LIMIT, 0, None).is_err());
    assert_eq!(out, [0x4a4a; 4]);
    // Enough concurrent edges to exceed the declared work cap. The fixture
    // remains small in pixels and input bytes; no giant allocation is needed.
    let mut points = Vec::new();
    let mut commands = Vec::new();
    for index in 0..9000 {
        let x = f64::from(index) / 9000.0;
        points.extend([[x, 0.0], [x + 0.5, 1.0]]);
        commands.extend([0, 1, 4]);
    }
    let mut out = [0x4a4a; 1];
    let error =
        coverage_request(&commands, &points, 1, 1, false, &mut out, LIMIT, 0, None).unwrap_err();
    assert!(error.contains("work"), "{error}");
    assert_eq!(out, [0x4a4a]);
}

#[test]
fn coverage_cancellation_before_and_during_work_is_atomic_and_releases_scratch() {
    let points = rectangle(0.13, 0.27, 63.71, 19_999.83);
    let commands = polygon_commands(&points);
    let cancelled = DrJobControl::default();
    cancelled.cancel();
    let mut output = [0x4a4a; 4];
    let error = coverage_request(
        &commands,
        &points,
        2,
        2,
        false,
        &mut output,
        LIMIT,
        0,
        Some(&cancelled),
    )
    .unwrap_err();
    assert!(error.contains("cancel"));
    assert_eq!(output, [0x4a4a; 4]);
    assert_eq!(cancelled.scratch_bytes(), 0);
    // MID-WORK: the default test parks the worker once, on rung 0 (a
    // 64 x 20 000 canvas), inside the coverage work, and cancels it there; the
    // cancelled request must publish nothing.
    let height = 20_000u32;
    let points = rectangle(0.13, 0.27, 63.71, f64::from(height) - 0.17);
    let commands = polygon_commands(&points);
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        let mut out = vec![0x4a4a; 64 * height as usize];
        match coverage_request(
            &commands,
            &points,
            64,
            height,
            false,
            &mut out,
            LIMIT,
            0,
            Some(control),
        ) {
            Ok(()) => {
                assert!(
                    out.iter().all(|value| *value != 0x4a4a),
                    "a completed request writes every sample"
                );
                (true, String::new())
            }
            Err(message) => {
                assert!(
                    out.iter().all(|value| *value == 0x4a4a),
                    "a cancelled request publishes nothing"
                );
                (false, message)
            }
        }
    };
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress > 0.0, render);
    let n0 = entry.checkpoints;
    let k = 1;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 1 + k,
        "the entry's kernel polls fewer than {} checkpoints before its next report (entry {n0}, next {next:?})",
        k + 1
    );
    let parked = cancelled_parked_at(
        n0 + 1 + k,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    println!("coverage cancellation: {}", parked_line(&parked));
    assert!(parked.progress > 0.0 && parked.progress < 1.0);
}

#[allow(clippy::too_many_arguments)]
fn render_request(
    description: &str,
    samples: &[u16],
    intrinsic: &[u8],
    width: u32,
    height: u32,
    profile: &IccProfile,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<Box<PrecisionImage>, String> {
    let description = CString::new(description).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_artwork_render(
            width,
            height,
            description.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            intrinsic.as_ptr(),
            intrinsic.len(),
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            limit,
            other,
            control.map_or(ptr::null(), |value| value),
            &mut error,
        )
    };
    let error = unsafe { take_text(error) };
    if result.is_null() {
        assert!(!error.is_empty(), "render refusal lacks diagnostic");
        Err(error)
    } else {
        assert!(error.is_empty(), "successful render reported {error}");
        Ok(unsafe { Box::from_raw(result) })
    }
}
fn render(description: &str, samples: &[u16], width: u32, height: u32) -> Box<PrecisionImage> {
    render_with_intrinsic(description, samples, &[], width, height)
}
fn render_with_intrinsic(
    description: &str,
    samples: &[u16],
    intrinsic: &[u8],
    width: u32,
    height: u32,
) -> Box<PrecisionImage> {
    render_request(
        description,
        samples,
        intrinsic,
        width,
        height,
        &IccProfile::srgb(),
        LIMIT,
        0,
        None,
    )
    .unwrap()
}
fn source_over(layers: &[[f64; 4]]) -> [u16; 4] {
    let mut premultiplied = [0.0; 3];
    let mut alpha = 0.0;
    for layer in layers {
        for channel in 0..3 {
            premultiplied[channel] =
                layer[channel] * layer[3] + premultiplied[channel] * (1.0 - layer[3]);
        }
        alpha = layer[3] + alpha * (1.0 - layer[3]);
    }
    let stored_alpha = area_code(alpha);
    if stored_alpha == 0 {
        return [0; 4];
    }
    [
        area_code(premultiplied[0] / alpha),
        area_code(premultiplied[1] / alpha),
        area_code(premultiplied[2] / alpha),
        stored_alpha,
    ]
}

#[test]
fn ordered_continuous_ink_and_intrinsic_byte_glyphs_match_independent_source_over() {
    let bottom = [0.123_456_f32, 0.612_345, 0.333_333, 0.71];
    let top = [0.879_123_f32, 0.223_456, 0.453_789, 0.43];
    let coverage_bottom = [65535, 17, 32768, 0, 65535];
    let coverage_top = [17777, 65535, 40001, 0, 11111];
    let glyphs: [[u8; 4]; 5] = [
        [40, 190, 120, 131],
        [210, 60, 90, 255],
        [0, 250, 30, 71],
        [255, 10, 244, 0],
        [80, 100, 120, 0],
    ];
    let mut samples = coverage_bottom.to_vec();
    samples.extend(coverage_top);
    let intrinsic: Vec<u8> = glyphs.iter().flatten().copied().collect();
    let description = json!([{"kind":"coverage","color":bottom}, {"kind":"intrinsic_rgba8"}, {"kind":"coverage","color":top}]).to_string();
    let actual = render_with_intrinsic(&description, &samples, &intrinsic, 5, 1);
    let actual = actual.to_rgba16().ok().unwrap();
    for index in 0..5 {
        let ink = |value: [f32; 4], coverage: u16| {
            [
                f64::from(value[0]),
                f64::from(value[1]),
                f64::from(value[2]),
                f64::from(value[3]) * f64::from(coverage) / 65535.0,
            ]
        };
        let glyph = glyphs[index].map(|value| f64::from(value) / 255.0);
        let layers = [
            ink(bottom, coverage_bottom[index]),
            glyph,
            ink(top, coverage_top[index]),
        ];
        let expected = source_over(&layers);
        for channel in 0..4 {
            assert!(
                actual[index][channel].abs_diff(expected[channel]) <= 1,
                "pixel{index} channel{channel}:{} !={}",
                actual[index][channel],
                expected[channel]
            );
        }
        if index == 0 {
            assert_ne!(expected, source_over(&[layers[2], layers[1], layers[0]]));
        }
    }
    assert_eq!(actual[3], [0; 4], "transparent intrinsic RGB leaked");
}

#[test]
fn authored_subbyte_color_and_native_coverage_survive_alongside_declared_byte_components() {
    let samples: Vec<u16> = (0..1024).map(|index| (index * 61 + 1) as u16).collect();
    let description =
        json!([{"kind":"coverage","color":[10001.0/65535.0,20002.0/65535.0,30003.0/65535.0,1.0]}])
            .to_string();
    let image = render(&description, &samples, 1024, 1);
    for (pixel, coverage) in image.to_rgba16().ok().unwrap().iter().zip(&samples) {
        assert_eq!(pixel[3], *coverage);
        for (channel, expected) in [10001_u16, 20002, 30003].iter().enumerate() {
            assert!(pixel[channel].abs_diff(*expected) <= 1);
        }
    }
    assert_eq!(
        image
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .map(|p| p[3])
            .collect::<HashSet<_>>()
            .len(),
        1024
    );
    let a = render_with_intrinsic(&json!([{"kind":"coverage","color":[10001.0/65535.0,0.5,0.5,1.0]}, {"kind":"intrinsic_rgba8"}]).to_string(), &[65535], &[0; 4], 1, 1);
    let b = render_with_intrinsic(&json!([{"kind":"coverage","color":[10003.0/65535.0,0.5,0.5,1.0]}, {"kind":"intrinsic_rgba8"}]).to_string(), &[65535], &[0; 4], 1, 1);
    assert_ne!(
        a.to_rgba16().ok(),
        b.to_rgba16().ok(),
        "intrinsic byte component quantized authored colour"
    );
}

#[test]
fn intrinsic_glyph_opacity_keeps_adjacent_continuous_native_alpha_values() {
    let intrinsic = [23_u8, 71, 109, 255];
    let mut alphas = Vec::new();
    for code in [32001.25_f64, 32002.25] {
        let opacity = (code / 65535.0) as f32;
        let description = json!([{"kind":"intrinsic_rgba8","opacity":opacity}]).to_string();
        let image = render_with_intrinsic(&description, &[], &intrinsic, 1, 1);
        let pixel = image.to_rgba16().ok().unwrap()[0];
        assert_eq!(pixel[3], area_code(f64::from(opacity)));
        for channel in 0..3 {
            assert!(pixel[channel].abs_diff(u16::from(intrinsic[channel]) * 257) <= 1);
        }
        alphas.push(pixel[3]);
    }
    assert_eq!(alphas, [32001, 32002]);
}

#[test]
fn rendering_refuses_malformed_intrinsic_layout_bad_components_and_resource_failures() {
    for (description, samples, intrinsic) in [
        (
            json!([{"kind":"intrinsic_rgba8"}]).to_string(),
            vec![],
            vec![1, 2, 3],
        ),
        (
            json!([{"kind":"coverage","color":[1.0,0.0,0.0,1.0],"unknown":1}]).to_string(),
            vec![65535],
            vec![],
        ),
        (
            json!([{"kind":"coverage","color":[1.1,0.0,0.0,1.0]}]).to_string(),
            vec![65535],
            vec![],
        ),
        (
            json!([{"kind":"coverage","color":[0.1,0.0,0.0,1e-30]}]).to_string(),
            vec![65535],
            vec![],
        ),
        (json!([]).to_string(), vec![65535], vec![]),
        (
            json!([{"kind":"coverage","color":[1.0,0.0,0.0,1.0]}]).to_string(),
            vec![65535, 65535],
            vec![],
        ),
    ] {
        assert!(render_request(
            &description,
            &samples,
            &intrinsic,
            1,
            1,
            &IccProfile::srgb(),
            LIMIT,
            0,
            None
        )
        .is_err());
    }
    let description = json!([{"kind":"coverage","color":[0.25,0.5,0.75,1.0]}]).to_string();
    for (limit, other) in [(1, 0), (LIMIT, LIMIT), (LIMIT, u64::MAX)] {
        assert!(render_request(
            &description,
            &[65535],
            &[],
            1,
            1,
            &IccProfile::srgb(),
            limit,
            other,
            None
        )
        .is_err());
    }
    let control = DrJobControl::default();
    control.cancel();
    assert!(render_request(
        &description,
        &[65535],
        &[],
        1,
        1,
        &IccProfile::srgb(),
        LIMIT,
        0,
        Some(&control)
    )
    .err()
    .unwrap()
    .contains("cancel"));
    assert_eq!(control.scratch_bytes(), 0);
}

#[test]
fn artwork_publication_keeps_continuous_master_and_description_with_semantic_noops() {
    let image = render(
        &json!([{"kind":"coverage","color":[0.123456,0.5,0.8,0.75]}]).to_string(),
        &[65535, 12345],
        2,
        1,
    );
    let profile = IccProfile::srgb();
    let blank = PrecisionImage::from_rgba16(4, 3, vec![[0; 4]; 12], profile, None).unwrap();
    let original =
        PrecisionDocument::from_image(blank, Metadata::default(), Resolution::default()).unwrap();
    let name = CString::new("Native ink").unwrap();
    let description =
        CString::new(r#"{"type":"shape","version":1,"probe":"native coverage"}"#).unwrap();
    let mut error = ptr::null_mut();
    let typed = unsafe {
        dr_precision_doc_artwork(
            &original,
            0,
            false,
            &*image,
            1,
            1,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { take_text(error) });
    assert!(!typed.is_null());
    let typed = unsafe { Box::from_raw(typed) };
    assert_eq!(typed.layer_count(), 2);
    assert_eq!(
        typed.layer(1).unwrap().source().unwrap().to_rgba16().ok(),
        image.to_rgba16().ok()
    );
    let read = unsafe { take_text(dr_precision_doc_artwork_description(&*typed, 1)) };
    assert_eq!(read, description.to_str().unwrap());
    let no_change = unsafe {
        dr_precision_doc_artwork(
            &*typed,
            1,
            true,
            &*image,
            1,
            1,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(no_change.is_null() && error.is_null());
    let unchanged_metadata = unsafe {
        dr_precision_doc_artwork_metadata(&*typed, 1, description.as_ptr(), LIMIT, 0, &mut error)
    };
    assert!(unchanged_metadata.is_null() && error.is_null());
    let cleared =
        unsafe { dr_precision_doc_artwork_metadata(&*typed, 1, ptr::null(), LIMIT, 0, &mut error) };
    assert!(error.is_null(), "{}", unsafe { take_text(error) });
    assert!(!cleared.is_null());
    let cleared = unsafe { Box::from_raw(cleared) };
    assert!(unsafe { dr_precision_doc_artwork_description(&*cleared, 1) }.is_null());
    assert_eq!(
        cleared.layer(1).unwrap().source().unwrap().to_rgba16().ok(),
        image.to_rgba16().ok()
    );
    let byte = DrDocument::from_pixels(RgbaImage::from_pixel(4, 3, Rgba([0; 4])));
    let result = unsafe {
        dr_doc_artwork(
            &byte,
            0,
            false,
            &*image,
            1,
            1,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { take_text(error) });
    assert!(!result.is_null());
    let result = unsafe { Box::from_raw(result) };
    assert_eq!(
        unsafe { take_text(dr_doc_layer_meta(&*result, 1)) },
        description.to_str().unwrap()
    );
}

#[test]
fn empty_optional_native_buffers_accept_null_only_when_their_lengths_are_zero() {
    let mut output = [0x4a4a; 2];
    let mut error = ptr::null_mut();
    assert!(unsafe {
        dr_artwork_path_coverage(
            ptr::null(),
            0,
            ptr::null(),
            0,
            2,
            1,
            false,
            output.as_mut_ptr(),
            2,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    });
    assert!(error.is_null(), "{}", unsafe { take_text(error) });
    assert_eq!(output, [0, 0]);
    let profile = IccProfile::srgb();
    let description = CString::new(r#"[{"kind":"intrinsic_rgba8"}]"#).unwrap();
    let glyph = [50, 100, 150, 255];
    let image = unsafe {
        dr_artwork_render(
            1,
            1,
            description.as_ptr(),
            ptr::null(),
            0,
            glyph.as_ptr(),
            glyph.len(),
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { take_text(error) });
    assert!(!image.is_null());
    let image = unsafe { Box::from_raw(image) };
    assert_eq!(
        image.to_rgba16().ok().unwrap()[0],
        [50 * 257, 100 * 257, 150 * 257, 65535]
    );
    let description = CString::new(r#"[{"kind":"coverage","color":[1,0,0,1]}]"#).unwrap();
    let coverage = [65535];
    let image = unsafe {
        dr_artwork_render(
            1,
            1,
            description.as_ptr(),
            coverage.as_ptr(),
            1,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { take_text(error) });
    assert!(!image.is_null());
    let image = unsafe { Box::from_raw(image) };
    assert_eq!(image.to_rgba16().ok().unwrap()[0], [65535, 0, 0, 65535]);
}

#[test]
fn every_artwork_export_tolerates_null_handles_and_error_outputs() {
    unsafe {
        assert!(!dr_artwork_path_coverage(
            ptr::null(),
            0,
            ptr::null(),
            0,
            1,
            1,
            false,
            ptr::null_mut(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(dr_artwork_render(
            1,
            1,
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_artwork(
            ptr::null(),
            0,
            false,
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_doc_artwork(
            ptr::null(),
            0,
            false,
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_artwork_description(ptr::null(), 0).is_null());
        assert!(dr_precision_doc_artwork_metadata(
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null_mut()
        )
        .is_null());
    }
}
