//! Auto-Align's feature registration through the FFI:
//! `dr_feature_register` on views of the committed photo
//! (`tests/fixtures/registration_photo/sky.jpg`, CC0) whose geometry is
//! known, checked against that geometry (the oracle is the map the views were
//! drawn with, never a recorded answer), and its refusals.
use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_feature_register::{dr_feature_register, dr_feature_register_scratch_bytes};

/// The worst distance from the truth a registration of a plain (lens-free)
/// view may have, frame pixels: the reach probe's plain bound
/// (`scripts/reach-probe/main.swift`).
const BOUND: f64 = 2.0;

struct Grey {
    width: usize,
    height: usize,
    values: Vec<f64>,
}

impl Grey {
    fn photo() -> Grey {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/registration_photo/sky.jpg"
        );
        let image = image::open(path)
            .expect("the fixture photo decodes")
            .to_luma8();
        Grey {
            width: image.width() as usize,
            height: image.height() as usize,
            values: image.pixels().map(|p| f64::from(p.0[0])).collect(),
        }
    }

    /// Bilinear at the continuous point (x, y), pixel i's centre at i + 0.5,
    /// clamped.
    fn sample(&self, x: f64, y: f64) -> f64 {
        let fx = (x - 0.5).clamp(0.0, (self.width - 1) as f64);
        let fy = (y - 0.5).clamp(0.0, (self.height - 1) as f64);
        let (x0, y0) = (
            (fx as usize).min(self.width - 2),
            (fy as usize).min(self.height - 2),
        );
        let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
        let at = |x: usize, y: usize| self.values[y * self.width + x];
        let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
        let bottom = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

/// A `width × height` view: pixel (x, y) shows photo point `source(centre)`.
fn view(
    photo: &Grey,
    width: usize,
    height: usize,
    source: impl Fn(f64, f64) -> (f64, f64),
) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let (px, py) = source(x as f64 + 0.5, y as f64 + 0.5);
            out.push(photo.sample(px, py).round().clamp(0.0, 255.0) as u8);
        }
    }
    out
}

/// The most inliers a call reports: the kernel's feature cap, as the header
/// states it.
const MAX_PAIRS: usize = 2000;

/// The registration, or its error message.
fn register(
    floating: &[u8],
    reference: &[u8],
    width: u32,
    height: u32,
) -> (Option<([f64; 9], u32)>, Option<String>) {
    let (fit, message) = register_with_pairs(floating, reference, width, height);
    (fit.map(|(map, inliers, _)| (map, inliers)), message)
}

/// The registration with its inlier pairs (floating x, y, reference x, y).
#[allow(clippy::type_complexity)]
fn register_with_pairs(
    floating: &[u8],
    reference: &[u8],
    width: u32,
    height: u32,
) -> (Option<([f64; 9], u32, Vec<[f64; 4]>)>, Option<String>) {
    let (fit, message) = register_with_lens(floating, reference, width, height);
    (
        fit.map(|(map, inliers, pairs, _)| (map, inliers, pairs)),
        message,
    )
}

/// The registration with its inlier pairs and the lens test's departure.
#[allow(clippy::type_complexity)]
fn register_with_lens(
    floating: &[u8],
    reference: &[u8],
    width: u32,
    height: u32,
) -> (Option<([f64; 9], u32, Vec<[f64; 4]>, f64)>, Option<String>) {
    let mut map = [0.0f64; 9];
    let mut pairs = vec![0.0f64; 4 * MAX_PAIRS];
    let mut inliers = 0u32;
    let mut departure = 0.0f64;
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_feature_register(
            floating.as_ptr(),
            reference.as_ptr(),
            width,
            height,
            map.as_mut_ptr(),
            pairs.as_mut_ptr(),
            MAX_PAIRS as u32,
            &mut inliers,
            &mut departure,
            &mut err,
        )
    };
    let message = (!err.is_null()).then(|| unsafe {
        let text = CStr::from_ptr(err).to_string_lossy().into_owned();
        dr_string_free(err);
        text
    });
    let pairs = pairs
        .chunks_exact(4)
        .take(inliers as usize)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    (ok.then_some((map, inliers, pairs, departure)), message)
}

fn apply(m: &[f64; 9], x: f64, y: f64) -> Option<(f64, f64)> {
    let w = m[6] * x + m[7] * y + m[8];
    (w > 0.0).then(|| {
        (
            (m[0] * x + m[1] * y + m[2]) / w,
            (m[3] * x + m[4] * y + m[5]) / w,
        )
    })
}

/// The worst distance of `map` from `truth` over a 33 × 33 grid of floating
/// points whose truth lands in the reference frame, and how many there were.
fn worst(
    map: &[f64; 9],
    width: usize,
    height: usize,
    truth: impl Fn(f64, f64) -> Option<(f64, f64)>,
) -> (f64, usize) {
    let (mut worst, mut count) = (0.0f64, 0);
    for j in 0..=32 {
        for i in 0..=32 {
            let x = 0.5 + i as f64 * (width - 1) as f64 / 32.0;
            let y = 0.5 + j as f64 * (height - 1) as f64 / 32.0;
            let Some(t) = truth(x, y) else { continue };
            if t.0 < 0.0 || t.0 > width as f64 || t.1 < 0.0 || t.1 > height as f64 {
                continue;
            }
            count += 1;
            worst = match apply(map, x, y) {
                Some(q) => worst.max((q.0 - t.0).hypot(q.1 - t.1)),
                None => f64::INFINITY,
            };
        }
    }
    (worst, count)
}

const ORIGIN: (f64, f64) = (724.0, 250.0);

/// A planar view: pixel p shows photo point ORIGIN + shift + c + A(p − c),
/// A turning by `turn` degrees, scaling by `scale` and tilting by (g, h).
fn planar(p: (f64, f64), shift: (f64, f64), turn: f64, scale: f64, tilt: (f64, f64)) -> (f64, f64) {
    let (cx, cy) = (300.0, 200.0);
    let (dx, dy) = (p.0 - cx, p.1 - cy);
    let (s, c) = turn.to_radians().sin_cos();
    let w = 1.0 + tilt.0 * dx + tilt.1 * dy;
    let (ax, ay) = (scale * (c * dx - s * dy) / w, scale * (s * dx + c * dy) / w);
    (ORIGIN.0 + shift.0 + cx + ax, ORIGIN.1 + shift.1 + cy + ay)
}

#[test]
fn a_planar_homography_on_the_photo_registers_within_two_pixels() {
    let photo = Grey::photo();
    let shift = (250.0, 30.0);
    let (turn, scale, tilt) = (4.0, 1.03, (2e-4, -1e-4));
    let reference = view(&photo, 600, 400, |x, y| (ORIGIN.0 + x, ORIGIN.1 + y));
    let floating = view(&photo, 600, 400, |x, y| {
        planar((x, y), shift, turn, scale, tilt)
    });
    let (fit, message) = register(&floating, &reference, 600, 400);
    assert_eq!(message, None);
    let (map, inliers) = fit.expect("a 58% overlap registers");
    assert!(inliers >= 24, "{inliers} inliers");
    let (error, points) = worst(&map, 600, 400, |x, y| {
        let p = planar((x, y), shift, turn, scale, tilt);
        Some((p.0 - ORIGIN.0, p.1 - ORIGIN.1))
    });
    assert!(points > 300, "{points} grid points in the overlap");
    assert!(error <= BOUND, "worst error {error:.3} px");
}

/// A pinhole view yawed `psi`, `width × height` at focal 600, inside the
/// probe's cylinder (`scripts/reach-probe/main.swift`).
fn cylinder(x: f64, y: f64, psi: f64, width: usize, height: usize) -> (f64, f64) {
    let (xc, yc) = (x - width as f64 / 2.0, y - height as f64 / 2.0);
    let f = 600.0;
    (
        1024.0 + f * (psi + xc.atan2(f)),
        450.0 + f * yc / xc.hypot(f),
    )
}

/// The truth between two such views `delta` apart: the rotation by `delta`
/// about the vertical axis through f, floating point to reference point.
fn yaw_truth(x: f64, y: f64, delta: f64, width: usize, height: usize) -> Option<(f64, f64)> {
    let (cx, cy) = (width as f64 / 2.0, height as f64 / 2.0);
    let (xc, yc, f) = (x - cx, y - cy, 600.0);
    let (s, c) = delta.sin_cos();
    let (rx, rz) = (xc * c + f * s, -xc * s + f * c);
    (rz > 0.0).then(|| (f * rx / rz + cx, f * yc / rz + cy))
}

#[test]
fn views_thirty_degrees_apart_register_within_two_pixels() {
    let photo = Grey::photo();
    let delta = 30f64.to_radians();
    let reference = view(&photo, 600, 400, |x, y| {
        cylinder(x, y, -delta / 2.0, 600, 400)
    });
    // A gain and an offset between the frames: the descriptors are
    // normalized, so they cancel.
    let floating: Vec<u8> = view(&photo, 600, 400, |x, y| {
        cylinder(x, y, delta / 2.0, 600, 400)
    })
    .into_iter()
    .map(|v| (0.8 * f64::from(v) + 20.0).round() as u8)
    .collect();
    let (fit, message) = register(&floating, &reference, 600, 400);
    assert_eq!(message, None);
    let (map, _) = fit.expect("a 44% overlap registers");
    let (error, points) = worst(&map, 600, 400, |x, y| yaw_truth(x, y, delta, 600, 400));
    assert!(points > 200, "{points} grid points in the overlap");
    assert!(error <= BOUND, "worst error {error:.3} px");
}

#[test]
fn small_frames_at_fifteen_percent_overlap_register_within_two_pixels() {
    // The reach probe's B-20-15 and B-30-15: W = round(2f·tan(Δ / 1.7)),
    // H = round(2W / 3). Their overlaps, about 37 and 57 px, are narrower
    // than level 0's margin on both sides plus a descriptor window; the
    // upsampled first level (`feature_points.rs`) halves both.
    let photo = Grey::photo();
    for (degrees, width, height) in [(20.0f64, 250usize, 167usize), (30.0, 382, 255)] {
        let delta = degrees.to_radians();
        let reference = view(&photo, width, height, |x, y| {
            cylinder(x, y, -delta / 2.0, width, height)
        });
        let floating = view(&photo, width, height, |x, y| {
            cylinder(x, y, delta / 2.0, width, height)
        });
        let (fit, message) = register(&floating, &reference, width as u32, height as u32);
        assert_eq!(message, None);
        let (map, inliers) = fit.unwrap_or_else(|| panic!("{width} × {height} at {degrees}°"));
        assert!(inliers >= 24, "{width} × {height}: {inliers} inliers");
        let (error, points) = worst(&map, width, height, |x, y| {
            yaw_truth(x, y, delta, width, height)
        });
        // 15 % of the 33 × 33 grid is about 160 points; a few dozen is the
        // floor that a shrunken overlap map would miss.
        assert!(
            points >= 40,
            "{width} × {height}: {points} grid points in the overlap"
        );
        assert!(
            error <= BOUND,
            "{width} × {height}: worst error {error:.3} px"
        );
    }
}

#[test]
fn the_same_call_answers_the_same_map() {
    let photo = Grey::photo();
    let reference = view(&photo, 400, 300, |x, y| (ORIGIN.0 + x, ORIGIN.1 + y));
    let floating = view(&photo, 400, 300, |x, y| {
        (ORIGIN.0 + 120.0 + x, ORIGIN.1 - 10.0 + y)
    });
    let first = register(&floating, &reference, 400, 300);
    let second = register(&floating, &reference, 400, 300);
    assert!(first.0.is_some());
    assert_eq!(first, second);
}

#[test]
fn unrelated_frames_have_no_estimate() {
    let photo = Grey::photo();
    // Two crops of the photo that share no pixel.
    let left = view(&photo, 600, 400, |x, y| (x, 100.0 + y));
    let right = view(&photo, 600, 400, |x, y| (1400.0 + x, 100.0 + y));
    assert_eq!(register(&left, &right, 600, 400), (None, None));
    // Flat grey has no corner at all.
    let flat = vec![128u8; 600 * 400];
    assert_eq!(register(&left, &flat, 600, 400), (None, None));
    assert_eq!(register(&flat, &flat, 600, 400), (None, None));
}

#[test]
fn a_size_outside_the_range_is_refused_with_a_message() {
    let small = vec![0u8; 15 * 15];
    let (fit, message) = register(&small, &small, 15, 15);
    assert!(fit.is_none());
    assert_eq!(
        message.as_deref(),
        Some("feature registration takes frames 16 to 4096 pixels a side, not 15 × 15")
    );
    let wide = vec![0u8; 4097 * 16];
    let (fit, message) = register(&wide, &wide, 4097, 16);
    assert!(fit.is_none());
    assert!(message.is_some_and(|m| m.ends_with("not 4097 × 16")));
    // The smallest size runs and finds nothing.
    let tiny = vec![7u8; 16 * 16];
    assert_eq!(register(&tiny, &tiny, 16, 16), (None, None));
}

#[test]
fn null_buffers_answer_false_with_no_error() {
    let frame = vec![0u8; 64 * 64];
    let mut map = [0.0f64; 9];
    for (a, b, m) in [
        (ptr::null(), frame.as_ptr(), map.as_mut_ptr()),
        (frame.as_ptr(), ptr::null(), map.as_mut_ptr()),
        (frame.as_ptr(), frame.as_ptr(), ptr::null_mut()),
    ] {
        let mut err: *mut c_char = ptr::null_mut();
        assert!(!unsafe {
            dr_feature_register(
                a,
                b,
                64,
                64,
                m,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut err,
            )
        });
        assert!(err.is_null());
    }
    // pairs_out, inliers_out, departure_out and err_out may be NULL on a
    // success too.
    let photo = Grey::photo();
    let reference = view(&photo, 400, 300, |x, y| (ORIGIN.0 + x, ORIGIN.1 + y));
    let floating = view(&photo, 400, 300, |x, y| {
        (ORIGIN.0 + 120.0 + x, ORIGIN.1 - 10.0 + y)
    });
    assert!(unsafe {
        dr_feature_register(
            floating.as_ptr(),
            reference.as_ptr(),
            400,
            300,
            map.as_mut_ptr(),
            ptr::null_mut(),
            MAX_PAIRS as u32,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    });
    assert_eq!(map[8], 1.0);
    assert!((map[2] - 120.0).abs() < BOUND && (map[5] + 10.0).abs() < BOUND);
}

#[test]
fn scratch_bytes_grow_with_the_frame_and_are_zero_outside_the_range() {
    let at = |w, h| unsafe { dr_feature_register_scratch_bytes(w, h) };
    assert_eq!(at(15, 100), 0);
    assert_eq!(at(100, 4097), 0);
    let small = at(16, 16);
    let analysis = at(2048, 1366);
    let largest = at(4096, 4096);
    assert!(small > 0);
    // At least the six level-0 f32 planes the detection holds.
    assert!(analysis >= 6 * 4 * 2048 * 1366);
    assert!(small < analysis && analysis < largest);
    // A frame whose longer side is under 512 px is detected on its 2×
    // upsampled level first: six f32 planes of four times its pixels.
    let upsampled = at(500, 300);
    assert!(upsampled >= 6 * 4 * (2 * 500) * (2 * 300));
    // The threshold itself: 511 px upsamples, 512 px does not.
    assert!(at(511, 300) > at(512, 300));
}

#[test]
fn the_pairs_are_the_inliers_and_true_matches() {
    let photo = Grey::photo();
    let shift = (250.0, 30.0);
    let (turn, scale, tilt) = (4.0, 1.03, (2e-4, -1e-4));
    let reference = view(&photo, 600, 400, |x, y| (ORIGIN.0 + x, ORIGIN.1 + y));
    let floating = view(&photo, 600, 400, |x, y| {
        planar((x, y), shift, turn, scale, tilt)
    });
    let (fit, message) = register_with_pairs(&floating, &reference, 600, 400);
    assert_eq!(message, None);
    let (map, inliers, pairs) = fit.expect("a 58% overlap registers");
    assert_eq!(pairs.len(), inliers as usize);
    assert!(inliers as usize <= MAX_PAIRS);
    // The header's inlier rule: each pair lands within 3.0 px under the map.
    // And each is a true match: within 3.0 px plus the bound of where the
    // drawn geometry puts it.
    for [fx, fy, rx, ry] in &pairs {
        let q = apply(&map, *fx, *fy).expect("an inlier maps");
        assert!((q.0 - rx).hypot(q.1 - ry) <= 3.0, "inlier off its map");
        let t = planar((*fx, *fy), shift, turn, scale, tilt);
        let (tx, ty) = (t.0 - ORIGIN.0, t.1 - ORIGIN.1);
        assert!(
            (tx - rx).hypot(ty - ry) <= 3.0 + BOUND,
            "a wrong match at ({fx}, {fy})"
        );
    }
}

#[test]
fn pairs_out_takes_no_more_than_its_capacity() {
    let photo = Grey::photo();
    let reference = view(&photo, 400, 300, |x, y| (ORIGIN.0 + x, ORIGIN.1 + y));
    let floating = view(&photo, 400, 300, |x, y| {
        (ORIGIN.0 + 120.0 + x, ORIGIN.1 - 10.0 + y)
    });
    let (full, _) = register_with_pairs(&floating, &reference, 400, 300);
    let (_, inliers, all) = full.expect("a 70% overlap registers");
    assert!(inliers > 5);
    // A buffer of five pairs and a sentinel after it.
    let mut buffer = vec![-7.0f64; 4 * 5 + 1];
    let mut map = [0.0f64; 9];
    let mut count = 0u32;
    assert!(unsafe {
        dr_feature_register(
            floating.as_ptr(),
            reference.as_ptr(),
            400,
            300,
            map.as_mut_ptr(),
            buffer.as_mut_ptr(),
            5,
            &mut count,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    });
    assert_eq!(count, inliers);
    assert_eq!(buffer[20], -7.0);
    let first: Vec<[f64; 4]> = buffer[..20]
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    assert_eq!(first, all[..5]);
}

/// The lens test's significance, pixels of departure (`AutoAlign.lensSignificance`,
/// rulings 2 to 4), at the worst inlier.
const SIGNIFICANCE: f64 = 1.0;

/// Where a point u of the ideal frame shows through the lens (the core's
/// convention): c + R·q·(1 + k|q|²), q = (u − c)/R.
fn undistort(p: (f64, f64), width: f64, height: f64, k: f64) -> (f64, f64) {
    let (cx, cy, radius) = (width / 2.0, height / 2.0, width.max(height) / 2.0);
    let (qx, qy) = ((p.0 - cx) / radius, (p.1 - cy) / radius);
    let rd = qx.hypot(qy);
    if rd == 0.0 {
        return p;
    }
    // Newton on r_d = r_u(1 + k r_u²).
    let mut ru = rd;
    for _ in 0..30 {
        ru -= (ru * (1.0 + k * ru * ru) - rd) / (1.0 + 3.0 * k * ru * ru);
    }
    let s = ru / rd;
    (cx + radius * qx * s, cy + radius * qy * s)
}

#[test]
fn the_lens_test_departs_on_a_shared_barrel_lens_and_not_on_a_plain_pair() {
    let photo = Grey::photo();
    let (w, h, k) = (600.0, 400.0, -0.05);
    let shift = (150.0, 20.0);
    // Both crops carry the drawn lens about their own centres: pixel p shows
    // the ideal crop at D⁻¹(p).
    let reference = view(&photo, 600, 400, |x, y| {
        let u = undistort((x, y), w, h, k);
        (ORIGIN.0 + u.0, ORIGIN.1 + u.1)
    });
    let floating = view(&photo, 600, 400, |x, y| {
        let u = undistort((x, y), w, h, k);
        (ORIGIN.0 + shift.0 + u.0, ORIGIN.1 + shift.1 + u.1)
    });
    let (fit, message) = register_with_lens(&floating, &reference, 600, 400);
    assert_eq!(message, None);
    let (_, _, _, found) = fit.expect("a 75% overlap through the lens registers");
    // A plain homography cannot follow the drawn lens over a 150 px shift:
    // the lens model departs from it past the 1.0 px significance.
    assert!(found >= SIGNIFICANCE, "departure {found:.3} px");
    // The same crops with no lens: the two models agree.
    let reference = view(&photo, 600, 400, |x, y| (ORIGIN.0 + x, ORIGIN.1 + y));
    let floating = view(&photo, 600, 400, |x, y| {
        (ORIGIN.0 + shift.0 + x, ORIGIN.1 + shift.1 + y)
    });
    let (fit, _) = register_with_lens(&floating, &reference, 600, 400);
    let (_, _, _, found) = fit.expect("a 75% overlap registers");
    assert!(found.is_finite());
    assert!(found < SIGNIFICANCE, "departure {found:.3} px");
    // Identical frames: every k predicts the same, so nothing departs
    // (ruling 3; the k alone is not pinned there).
    let (fit, _) = register_with_lens(&reference, &reference, 600, 400);
    let (_, _, _, found) = fit.expect("identical frames register");
    assert!(found.is_finite());
    assert!(found < SIGNIFICANCE, "departure {found:.3} px");
}

#[test]
fn the_lens_test_departs_on_a_narrow_overlap_through_a_shared_lens() {
    // The reach probe's B-20-30: 306 × 204 views 20° apart (30 % overlap).
    // On level 0 its inliers sat in a strip a few dozen pixels wide, inside
    // the 27 px margin, where the drawn lens and a plain homography still
    // agree (0.70 px); the upsampled level's half margin lets them reach
    // toward the frame's edge, where they do not.
    let photo = Grey::photo();
    let (width, height, k) = (306usize, 204usize, -0.05);
    let delta = 20f64.to_radians();
    let (w, h) = (width as f64, height as f64);
    let lensed = |psi: f64| {
        view(&photo, width, height, |x, y| {
            let u = undistort((x, y), w, h, k);
            cylinder(u.0, u.1, psi, width, height)
        })
    };
    let (fit, message) = register_with_lens(
        &lensed(delta / 2.0),
        &lensed(-delta / 2.0),
        width as u32,
        height as u32,
    );
    assert_eq!(message, None);
    let (_, _, _, found) = fit.expect("a 30% overlap through the lens registers");
    assert!(found >= SIGNIFICANCE, "departure {found:.3} px");
    // The same views with no lens: the two models agree.
    let plain = |psi: f64| {
        view(&photo, width, height, |x, y| {
            cylinder(x, y, psi, width, height)
        })
    };
    let (fit, message) = register_with_lens(
        &plain(delta / 2.0),
        &plain(-delta / 2.0),
        width as u32,
        height as u32,
    );
    assert_eq!(message, None);
    let (_, _, _, found) = fit.expect("a 30% overlap registers");
    assert!(found.is_finite());
    assert!(found < SIGNIFICANCE, "departure {found:.3} px");
}
