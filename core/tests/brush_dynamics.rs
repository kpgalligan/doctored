//! Brush dynamics (`brush_dynamics`, `designs/brush-dynamics.md`), black-box
//! through the C ABI against an oracle written here from the rule alone: its
//! own SplitMix64 (Steele/Lea/Flood's finalizer over the documented positional
//! seed), its own natural cubic in 0…1 solved by plain Gaussian elimination,
//! and the rule's formulas in f64.
use crate::native_scalar::{error, text};
use doctored_core::ffi_brush_dynamics::*;
use serde_json::{json, Value};
use std::ptr;

struct Handle(*mut DrBrushDynamics);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { dr_brush_dynamics_free(self.0) }
    }
}

fn create(request: Value) -> Result<Handle, String> {
    let t = text(request);
    let mut e = ptr::null_mut();
    let h = unsafe { dr_brush_dynamics_create(t.as_ptr(), &mut e) };
    if h.is_null() {
        return Err(unsafe { error(e) });
    }
    assert!(e.is_null());
    Ok(Handle(h))
}

fn handle(request: Value) -> Handle {
    create(request).unwrap()
}

const NAN: f64 = f64::NAN;

/// x, y, size, pressure, tilt_x, tilt_y, dir_x, dir_y; no rotation, the
/// heading the direction.
fn sample(
    x: f64,
    y: f64,
    size: f64,
    pressure: f64,
    tilt: (f64, f64),
    dir: (f64, f64),
) -> [f64; 11] {
    pen(x, y, size, pressure, tilt, dir, NAN, dir)
}

/// The full sample: `sample`'s values, the rotation in degrees and the heading.
#[allow(clippy::too_many_arguments)]
fn pen(
    x: f64,
    y: f64,
    size: f64,
    pressure: f64,
    tilt: (f64, f64),
    dir: (f64, f64),
    rotation: f64,
    heading: (f64, f64),
) -> [f64; 11] {
    [
        x, y, size, pressure, tilt.0, tilt.1, dir.0, dir.1, rotation, heading.0, heading.1,
    ]
}

/// Each dab's first four values (slice 1's tuple).
fn dabs(h: &Handle, index: u64, s: [f64; 11]) -> Vec<[f64; 4]> {
    shaped(h, index, s)
        .into_iter()
        .map(|d| [d[0], d[1], d[2], d[3]])
        .collect()
}

/// Each dab's six values: slice 1's four, then its angle and roundness.
fn shaped(h: &Handle, index: u64, s: [f64; 11]) -> Vec<[f64; 6]> {
    let mut out = vec![-7.0; 128];
    let n =
        unsafe { dr_brush_dynamics_dabs(h.0, index, s.as_ptr(), 11, out.as_mut_ptr(), out.len()) };
    assert!(n >= 1, "no dabs for {s:?}");
    assert!(
        out[6 * n..].iter().all(|&v| v == -7.0),
        "wrote past its dabs"
    );
    out[..6 * n]
        .chunks(6)
        .map(|c| [c[0], c[1], c[2], c[3], c[4], c[5]])
        .collect()
}

fn nominal(h: &Handle, size: f64, pressure: f64, tilt: (f64, f64)) -> (f64, f64) {
    let mut out = [0.0; 2];
    assert!(unsafe {
        dr_brush_dynamics_nominal(h.0, size, pressure, tilt.0, tilt.1, out.as_mut_ptr())
    });
    (out[0], out[1])
}

// ---- the oracle ---------------------------------------------------------

/// SplitMix64 at the positional seed `seed ^ a·γ ^ b·m1 ^ c·m2`, γ the Weyl
/// increment and m1, m2 the finalizer's multipliers, drawing unit values from
/// the top 53 bits.
struct Draws(u64);
impl Draws {
    fn at(seed: u64, a: u64, b: u64, c: u64) -> Self {
        Draws(
            seed ^ a.wrapping_mul(0x9E3779B97F4A7C15)
                ^ b.wrapping_mul(0xBF58476D1CE4E5B9)
                ^ c.wrapping_mul(0x94D049BB133111EB),
        )
    }
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        (z >> 11) as f64 / 9007199254740992.0
    }
}

/// The natural cubic through `points` at `x`: the second derivatives from the
/// full tridiagonal system by Gaussian elimination, then the segment's cubic.
fn natural_cubic(points: &[[f64; 2]], x: f64) -> f64 {
    let n = points.len();
    let h: Vec<f64> = (0..n - 1)
        .map(|k| points[k + 1][0] - points[k][0])
        .collect();
    let mut a = vec![vec![0.0; n + 1]; n];
    a[0][0] = 1.0;
    a[n - 1][n - 1] = 1.0;
    for i in 1..n - 1 {
        a[i][i - 1] = h[i - 1];
        a[i][i] = 2.0 * (h[i - 1] + h[i]);
        a[i][i + 1] = h[i];
        a[i][n] = 6.0
            * ((points[i + 1][1] - points[i][1]) / h[i]
                - (points[i][1] - points[i - 1][1]) / h[i - 1]);
    }
    for col in 0..n {
        for row in col + 1..n {
            let f = a[row][col] / a[col][col];
            let (top, bottom) = a.split_at_mut(row);
            for (x, p) in bottom[0][col..].iter_mut().zip(&top[col][col..]) {
                *x -= f * p;
            }
        }
    }
    let mut m = vec![0.0; n];
    for row in (0..n).rev() {
        let mut s = a[row][n];
        for k in row + 1..n {
            s -= a[row][k] * m[k];
        }
        m[row] = s / a[row][row];
    }
    let k = (0..n - 1).find(|&k| x <= points[k + 1][0]).unwrap_or(n - 2);
    let (x0, y0, x1, y1) = (
        points[k][0],
        points[k][1],
        points[k + 1][0],
        points[k + 1][1],
    );
    let t = x1 - x0;
    let p = (x1 - x) / t;
    let q = (x - x0) / t;
    let v = p * y0 + q * y1 + ((p * p * p - p) * m[k] + (q * q * q - q) * m[k + 1]) * t * t / 6.0;
    v.clamp(0.0, 1.0)
}

const SIZE: f64 = 40.0;

// ---- cases ----------------------------------------------------------------

#[test]
fn pressure_controls_lay_one_dab_at_the_identity() {
    let h =
        handle(json!({"version":1,"size":{"control":"pressure"},"opacity":{"control":"pressure"}}));
    for p in [0.0, 0.01, 0.25, 0.5, 0.8, 1.0] {
        let got = dabs(&h, 3, sample(10.25, -4.5, SIZE, p, (NAN, NAN), (1.0, 0.0)));
        assert_eq!(got, vec![[10.25, -4.5, (SIZE * p).max(1.0), p]]);
        assert_eq!(nominal(&h, SIZE, p, (NAN, NAN)), ((SIZE * p).max(1.0), p));
    }
}

#[test]
fn a_curve_shapes_the_response_at_and_between_its_knots() {
    let points = [[0.0, 0.0], [0.3, 0.6], [0.7, 0.65], [1.0, 1.0]];
    let h = handle(
        json!({"version":1,"size":{"control":"pressure","curve":points},
                          "opacity":{"control":"pressure","curve":points}}),
    );
    for p in [0.0, 0.1, 0.3, 0.45, 0.7, 0.9, 1.0] {
        let c = natural_cubic(&points, p);
        let got = dabs(&h, 0, sample(5.0, 5.0, SIZE, p, (NAN, NAN), (NAN, NAN)));
        assert_eq!(got.len(), 1);
        assert!(
            (got[0][2] - (SIZE * c).max(1.0)).abs() < 1e-9,
            "{p}: {got:?} vs {c}"
        );
        assert!((got[0][3] - c).abs() < 1e-11, "{p}: {got:?} vs {c}");
    }
    // A bent curve differs from the identity between its knots.
    assert!((natural_cubic(&points, 0.15) - 0.15).abs() > 0.05);
}

#[test]
fn the_minimum_lifts_the_factor_and_tilt_can_be_the_control() {
    let h = handle(json!({"version":1,"size":{"control":"tilt","minimum":0.25},
                          "opacity":{"control":"pressure","minimum":0.5}}));
    for (tilt, p) in [
        ((0.3, 0.4), 0.2),
        ((0.0, 0.0), 1.0),
        ((1.0, 1.0), 0.0),
        ((-0.6, 0.0), 0.5),
    ] {
        let v = f64::hypot(tilt.0, tilt.1).min(1.0);
        let (d, o) = nominal(&h, SIZE, p, tilt);
        assert!(
            (d - (SIZE * (0.25 + 0.75 * v)).max(1.0)).abs() < 1e-12,
            "{tilt:?}"
        );
        assert!((o - (0.5 + 0.5 * p)).abs() < 1e-12);
    }
}

#[test]
fn a_stamp_without_a_sample_reads_its_control_as_off() {
    let h = handle(json!({"version":1,"size":{"control":"tilt","minimum":0.1},
                          "opacity":{"control":"pressure","minimum":0.2}}));
    assert_eq!(nominal(&h, SIZE, NAN, (NAN, NAN)), (SIZE, 1.0));
    assert_eq!(
        nominal(&h, SIZE, 0.5, (NAN, 0.0)),
        (SIZE, 0.2 + (1.0 - 0.2) * 0.5)
    );
    assert_eq!(nominal(&h, SIZE, NAN, (0.0, f64::INFINITY)), (SIZE, 1.0));
    // Off ignores the sample it has.
    let off =
        handle(json!({"version":1,"size":{"control":"off","minimum":0.5,"curve":[[0,0],[1,0]]}}));
    assert_eq!(nominal(&off, SIZE, 0.1, (0.2, 0.2)), (SIZE, 1.0));
    // The diameter is floored at one pixel.
    let low = handle(json!({"version":1,"size":{"control":"pressure"}}));
    assert_eq!(nominal(&low, SIZE, 0.0, (NAN, NAN)), (1.0, 1.0));
}

#[test]
fn size_and_opacity_jitter_follow_the_seeded_draws() {
    for seed in [0u64, 7, 4_294_967_295] {
        let h = handle(json!({"version":1,"seed":seed,"size":{"jitter":0.6},
                              "opacity":{"control":"pressure","jitter":0.3}}));
        let mut distinct = std::collections::BTreeSet::new();
        for i in 0..64u64 {
            let got = dabs(&h, i, sample(1.0, 2.0, SIZE, 0.8, (NAN, NAN), (0.0, 1.0)));
            let mut u = Draws::at(seed, i, 0, 2);
            let (u1, u2) = (u.unit(), u.unit());
            let want = [
                1.0,
                2.0,
                (SIZE * (1.0 - 0.6 * u1)).max(1.0),
                0.8 * (1.0 - 0.3 * u2),
            ];
            assert_eq!(got, vec![want], "seed {seed} stamp {i}");
            assert!(got[0][2] <= SIZE && got[0][2] >= SIZE * 0.4);
            distinct.insert(got[0][2].to_bits());
        }
        assert!(distinct.len() > 60, "the draws vary");
    }
}

#[test]
fn scatter_runs_across_the_direction_of_travel_and_along_it_with_both_axes() {
    let dir = (3.0, 4.0); // not unit: the kernel normalizes it
    let (dx, dy) = (0.6, 0.8);
    for both in [false, true] {
        let h = handle(json!({"version":1,"seed":11,"size":{"control":"pressure"},
                              "scatter":{"amount":1.5,"both_axes":both}}));
        let d0 = SIZE * 0.5;
        for i in 0..32u64 {
            let got = dabs(&h, i, sample(100.0, 50.0, SIZE, 0.5, (NAN, NAN), dir));
            assert_eq!(got.len(), 1);
            let mut u = Draws::at(11, i, 0, 2);
            let (_, _, u3, u4) = (u.unit(), u.unit(), u.unit(), u.unit());
            let across = 1.5 * d0 * (2.0 * u3 - 1.0);
            let along = if both {
                1.5 * d0 * (2.0 * u4 - 1.0)
            } else {
                0.0
            };
            let want = (
                100.0 - dy * across + dx * along,
                50.0 + dx * across + dy * along,
            );
            assert!((got[0][0] - want.0).abs() < 1e-9 && (got[0][1] - want.1).abs() < 1e-9);
            // The offset's component along the travel is zero without both axes.
            let along_got = (got[0][0] - 100.0) * dx + (got[0][1] - 50.0) * dy;
            if !both {
                assert!(along_got.abs() < 1e-9, "{along_got}");
            }
            assert!(
                f64::hypot(got[0][0] - 100.0, got[0][1] - 50.0) <= 1.5 * d0 * 2f64.sqrt() + 1e-9
            );
            assert_eq!(got[0][2], d0);
        }
    }
}

#[test]
fn scatter_without_a_direction_takes_a_seeded_angle_and_zero_keeps_the_point() {
    let h = handle(json!({"version":1,"seed":5,"scatter":{"amount":2}}));
    for i in 0..16u64 {
        let got = dabs(&h, i, sample(7.0, 9.0, SIZE, NAN, (NAN, NAN), (0.0, 0.0)));
        let mut u = Draws::at(5, i, 0, 2);
        let (_, _, u3, u4) = (u.unit(), u.unit(), u.unit(), u.unit());
        let r = 2.0 * SIZE * (2.0 * u3 - 1.0);
        let a = std::f64::consts::TAU * u4;
        assert!((got[0][0] - (7.0 + a.cos() * r)).abs() < 1e-9);
        assert!((got[0][1] - (9.0 + a.sin() * r)).abs() < 1e-9);
    }
    let none = handle(json!({"version":1,"seed":5,"scatter":{"amount":0,"count":3}}));
    let x = 0.1 + 0.2; // not a round number: the position must come back bit for bit
    for got in dabs(&none, 4, sample(x, -x, SIZE, NAN, (NAN, NAN), (1.0, 1.0))) {
        assert_eq!(
            (got[0].to_bits(), got[1].to_bits()),
            (x.to_bits(), (-x).to_bits())
        );
    }
}

#[test]
fn count_and_count_jitter_set_the_dabs_per_stamp() {
    let h = handle(json!({"version":1,"seed":9,"scatter":{"amount":1,"count":16}}));
    let got = dabs(&h, 2, sample(0.0, 0.0, SIZE, NAN, (NAN, NAN), (1.0, 0.0)));
    assert_eq!(got.len(), 16);
    for (k, dab) in got.iter().enumerate() {
        let mut u = Draws::at(9, 2, k as u64, 2);
        let (_, _, u3, _) = (u.unit(), u.unit(), u.unit(), u.unit());
        assert!((dab[1] - SIZE * (2.0 * u3 - 1.0)).abs() < 1e-9, "dab {k}");
    }
    let jittered = handle(json!({"version":1,"seed":9,"scatter":{"count":10,"count_jitter":0.8}}));
    let mut counts = std::collections::BTreeSet::new();
    for i in 0..64u64 {
        let n = dabs(
            &jittered,
            i,
            sample(0.0, 0.0, SIZE, NAN, (NAN, NAN), (NAN, NAN)),
        )
        .len();
        let u = Draws::at(9, i, 0, 1).unit();
        assert_eq!(
            n,
            ((10.0 * (1.0 - 0.8 * u)).round() as usize).max(1),
            "stamp {i}"
        );
        counts.insert(n);
    }
    assert!(counts.len() >= 5 && counts.iter().all(|&n| (2..=10).contains(&n)));
}

#[test]
fn a_stamp_resolves_alone_as_it_does_inside_a_walk_and_the_seed_matters() {
    let request = json!({"version":1,"seed":21,"size":{"jitter":1},"opacity":{"jitter":1},
                         "scatter":{"amount":3,"both_axes":true,"count":4,"count_jitter":0.5}});
    let walk = handle(request.clone());
    let s = |i: u64| sample(i as f64, 2.0 * i as f64, SIZE, 0.7, (0.1, 0.2), (1.0, -1.0));
    let walked: Vec<_> = (0..40u64).map(|i| dabs(&walk, i, s(i))).collect();
    let fresh = handle(request.clone());
    for i in (0..40u64).rev() {
        assert_eq!(dabs(&fresh, i, s(i)), walked[i as usize], "stamp {i}");
    }
    let mut other = request;
    other["seed"] = json!(22);
    let other = handle(other);
    assert_ne!(dabs(&other, 5, s(5)), walked[5]);
}

/// The oracle's angle wrap into (−180, 180], by whole turns.
fn wrapped(mut degrees: f64) -> f64 {
    while degrees > 180. {
        degrees -= 360.;
    }
    while degrees <= -180. {
        degrees += 360.;
    }
    degrees
}

/// The oracle's fifth and sixth draws of dab `k` of stamp `i`.
fn shape_draws(seed: u64, i: u64, k: u64) -> (f64, f64) {
    let mut u = Draws::at(seed, i, k, 2);
    for _ in 0..4 {
        u.unit();
    }
    (u.unit(), u.unit())
}

/// The rule's direction angle of a vector, degrees counter-clockwise from +x in
/// a y-up frame; 0 for none.
fn y_up_angle(x: f64, y: f64) -> f64 {
    if x == 0. && y == 0. {
        0.
    } else {
        y.atan2(x).to_degrees()
    }
}

#[test]
fn the_angle_follows_rotation_tilt_and_heading_with_its_seeded_jitter() {
    let tip = -30.0;
    for (control, jitter) in [
        ("off", 0.4),
        ("rotation", 0.0),
        ("rotation", 0.25),
        ("tilt", 0.5),
        ("direction", 0.0),
        ("direction", 1.0),
    ] {
        let h = handle(json!({"version":1,"seed":17,"scatter":{"count":3},
            "tip":{"angle_degrees":tip,"roundness":0.4},
            "angle":{"control":control,"jitter":jitter}}));
        let cases = [
            (NAN, (NAN, NAN), (0.0, 0.0)),
            (200.0, (0.3, -0.4), (3.0, 4.0)),
            (-75.5, (-0.6, 0.0), (0.0, 2.0)),
            (10.0, (0.0, 0.0), (-1.0, -1.0)),
        ];
        for (i, (rotation, tilt, heading)) in cases.into_iter().enumerate() {
            let i = i as u64;
            // The direction of travel is the heading's opposite: the angle reads the heading.
            let dir = (-heading.0, -heading.1);
            let s = pen(5.0, 6.0, SIZE, 0.5, tilt, dir, rotation, heading);
            let a = match control {
                "rotation" if rotation.is_finite() => rotation,
                "tilt" if tilt.0.is_finite() => y_up_angle(tilt.0, tilt.1),
                // Canvas y runs down, so a heading (x, y) is (x, −y) in the y-up frame.
                "direction" => y_up_angle(heading.0, -heading.1),
                _ => 0.,
            };
            for (k, dab) in shaped(&h, i, s).into_iter().enumerate() {
                let (u5, _) = shape_draws(17, i, k as u64);
                let want = wrapped(tip + a + 180. * jitter * (2. * u5 - 1.));
                assert!(
                    (dab[4] - want).abs() < 1e-9 && dab[4] > -180. && dab[4] <= 180.,
                    "{control} {jitter} stamp {i} dab {k}: {} vs {want}",
                    dab[4]
                );
                assert_eq!(dab[5], 0.4, "no roundness section keeps the tip's");
            }
        }
    }
    // A stamp at rest (no direction) keeps the heading's angle; the first stamp (neither)
    // lays at the tip's own angle.
    let h = handle(json!({"version":1,"tip":{"angle_degrees":20},"angle":{"control":"direction"}}));
    let rest = pen(1.0, 1.0, SIZE, NAN, (NAN, NAN), (0.0, 0.0), NAN, (0.0, 5.0));
    assert!((shaped(&h, 3, rest)[0][4] - (20. - 90.)).abs() < 1e-12);
    let first = pen(1.0, 1.0, SIZE, NAN, (NAN, NAN), (0.0, 0.0), NAN, (0.0, 0.0));
    assert_eq!(shaped(&h, 0, first)[0][4], 20.);
}

#[test]
fn roundness_follows_its_control_and_minimum_and_shrinks_by_its_jitter() {
    for (control, minimum, jitter) in [
        ("off", 0.0, 0.0),
        ("off", 0.3, 0.5),
        ("pressure", 0.25, 0.0),
        ("pressure", 0.5, 0.6),
        ("tilt", 0.1, 0.2),
    ] {
        let h = handle(json!({"version":1,"seed":4,"tip":{"roundness":0.8},
            "roundness":{"control":control,"minimum":minimum,"jitter":jitter}}));
        for (i, (p, tilt)) in [
            (0.0, (0.3, 0.4)),
            (0.5, (1.0, 1.0)),
            (1.0, (NAN, NAN)),
            (NAN, (0.0, 0.0)),
        ]
        .into_iter()
        .enumerate()
        {
            let i = i as u64;
            let v = match control {
                "pressure" => p,
                "tilt" => f64::hypot(tilt.0, tilt.1).min(1.),
                _ => NAN,
            };
            let f = if v.is_finite() {
                minimum + (1. - minimum) * v
            } else {
                1.
            };
            let (_, u6) = shape_draws(4, i, 0);
            let want = (0.8 * f * (1. - jitter * u6)).clamp(0.01, 1.);
            let got = shaped(
                &h,
                i,
                pen(0.0, 0.0, SIZE, p, tilt, (1.0, 0.0), NAN, (1.0, 0.0)),
            );
            assert!(
                (got[0][5] - want).abs() < 1e-12,
                "{control} {i}: {} vs {want}",
                got[0][5]
            );
            assert_eq!(got[0][4], 0., "no angle section keeps the tip's");
        }
    }
    // The floor: a narrow tip at zero pressure stays at 0.01.
    let h =
        handle(json!({"version":1,"tip":{"roundness":0.01},"roundness":{"control":"pressure"}}));
    let got = shaped(
        &h,
        0,
        pen(0.0, 0.0, SIZE, 0.0, (NAN, NAN), (0.0, 0.0), NAN, (0.0, 0.0)),
    );
    assert_eq!(got[0][5], 0.01);
}

#[test]
fn the_tip_shape_leaves_slice_one_dabs_as_they_were() {
    let base = json!({"version":1,"seed":33,"size":{"jitter":0.7},"opacity":{"jitter":0.2},
        "scatter":{"amount":2,"both_axes":true,"count":5,"count_jitter":0.4}});
    let mut shaped_request = base.clone();
    shaped_request["tip"] = json!({"angle_degrees":45,"roundness":0.5});
    shaped_request["angle"] = json!({"control":"direction","jitter":0.5});
    shaped_request["roundness"] = json!({"control":"pressure","minimum":0.2,"jitter":0.3});
    let (plain, with_shape) = (handle(base), handle(shaped_request));
    for i in 0..24u64 {
        let s = pen(
            i as f64,
            3.0,
            SIZE,
            0.6,
            (0.2, 0.1),
            (1.0, 2.0),
            40.0,
            (2.0, 1.0),
        );
        assert_eq!(dabs(&plain, i, s), dabs(&with_shape, i, s), "stamp {i}");
        // Without the sections a dab lies at angle 0 and roundness 1.
        assert!(shaped(&plain, i, s)
            .iter()
            .all(|d| d[4] == 0. && d[5] == 1.));
    }
}

#[test]
fn malformed_requests_are_refused_by_name() {
    let cases = [
        (json!({"version":2}), "version 1"),
        (
            json!({"version":1,"seed":4_294_967_296u64}),
            "invalid brush dynamics request",
        ),
        (
            json!({"version":1,"seed":-1}),
            "invalid brush dynamics request",
        ),
        (
            json!({"version":1,"spread":1}),
            "invalid brush dynamics request",
        ),
        (
            json!({"version":1,"size":{"control":"rotation"}}),
            "invalid brush dynamics request",
        ),
        (
            json!({"version":1,"size":{"minimum":1.5}}),
            "size minimum and jitter",
        ),
        (
            json!({"version":1,"opacity":{"jitter":-0.1}}),
            "opacity minimum and jitter",
        ),
        (
            json!({"version":1,"size":{"curve":[[0,0]]}}),
            "size curve holds 2 to 16",
        ),
        (
            json!({"version":1,"size":{"curve":[[0,0],[0.6,1],[0.5,1],[1,1]]}}),
            "size curve",
        ),
        (
            json!({"version":1,"size":{"curve":[[0.1,0],[1,1]]}}),
            "size curve",
        ),
        (
            json!({"version":1,"opacity":{"curve":[[0,0],[0.9,1]]}}),
            "opacity curve",
        ),
        (
            json!({"version":1,"size":{"curve":[[0,0],[1,1.2]]}}),
            "size curve",
        ),
        (
            json!({"version":1,"scatter":{"amount":10.5}}),
            "scatter amount",
        ),
        (json!({"version":1,"scatter":{"count":17}}), "scatter count"),
        (json!({"version":1,"scatter":{"count":0}}), "scatter count"),
        (
            json!({"version":1,"scatter":{"count_jitter":2}}),
            "count_jitter",
        ),
        (
            json!({"version":1,"angle":{"control":"pressure"}}),
            "invalid brush dynamics request",
        ),
        (json!({"version":1,"angle":{"jitter":1.5}}), "angle jitter"),
        (
            json!({"version":1,"roundness":{"minimum":2}}),
            "roundness minimum and jitter",
        ),
        (
            json!({"version":1,"roundness":{"curve":[[0,0],[1,1]]}}),
            "invalid brush dynamics request",
        ),
        (
            json!({"version":1,"roundness":{"control":"rotation"}}),
            "invalid brush dynamics request",
        ),
        (
            json!({"version":1,"tip":{"roundness":0.005}}),
            "tip holds an angle_degrees",
        ),
        (
            json!({"version":1,"tip":{"angle_degrees":181}}),
            "tip holds an angle_degrees",
        ),
    ];
    for (request, needle) in cases {
        let e = create(request.clone())
            .err()
            .unwrap_or_else(|| panic!("{request} was accepted"));
        assert!(e.contains(needle), "{request}: {e}");
    }
    let many: Vec<[f64; 2]> = (0..17).map(|k| [k as f64 / 16.0, 0.5]).collect();
    let e = create(json!({"version":1,"size":{"curve":many}}))
        .err()
        .unwrap();
    assert!(e.contains("size curve holds 2 to 16"), "{e}");
    let sixteen: Vec<[f64; 2]> = (0..16).map(|k| [k as f64 / 15.0, 0.5]).collect();
    assert!(create(json!({"version":1,"size":{"curve":sixteen}})).is_ok());
    let padded = json!({"version":1,"seed":0,"pad":"x".repeat(5000)});
    let e = create(padded).err().unwrap();
    assert!(e.contains("exceeds"), "{e}");
}

#[test]
fn null_inputs_short_buffers_and_bad_samples_write_nothing() {
    let h = handle(json!({"version":1,"scatter":{"count":3}}));
    let s = sample(1.0, 1.0, SIZE, 1.0, (0.0, 0.0), (1.0, 0.0));
    let mut out = [-7.0; 64];
    unsafe {
        let mut e = ptr::null_mut();
        assert!(dr_brush_dynamics_create(ptr::null(), &mut e).is_null());
        assert!(e.is_null());
        dr_brush_dynamics_free(ptr::null_mut());
        assert!(!dr_brush_dynamics_nominal(
            ptr::null(),
            1.0,
            1.0,
            0.0,
            0.0,
            out.as_mut_ptr()
        ));
        assert!(!dr_brush_dynamics_nominal(
            h.0,
            1.0,
            1.0,
            0.0,
            0.0,
            ptr::null_mut()
        ));
        assert!(!dr_brush_dynamics_nominal(
            h.0,
            NAN,
            1.0,
            0.0,
            0.0,
            out.as_mut_ptr()
        ));
        assert!(!dr_brush_dynamics_nominal(
            h.0,
            -1.0,
            1.0,
            0.0,
            0.0,
            out.as_mut_ptr()
        ));
        assert_eq!(
            dr_brush_dynamics_dabs(ptr::null(), 0, s.as_ptr(), 11, out.as_mut_ptr(), 64),
            0
        );
        assert_eq!(
            dr_brush_dynamics_dabs(h.0, 0, ptr::null(), 11, out.as_mut_ptr(), 64),
            0
        );
        assert_eq!(
            dr_brush_dynamics_dabs(h.0, 0, s.as_ptr(), 11, ptr::null_mut(), 64),
            0
        );
        // Slice 1's eight-value sample is refused.
        assert_eq!(
            dr_brush_dynamics_dabs(h.0, 0, s.as_ptr(), 8, out.as_mut_ptr(), 64),
            0
        );
        assert_eq!(
            dr_brush_dynamics_dabs(h.0, 0, s.as_ptr(), 11, out.as_mut_ptr(), 17),
            0
        );
        let bad = sample(NAN, 1.0, SIZE, 1.0, (0.0, 0.0), (1.0, 0.0));
        assert_eq!(
            dr_brush_dynamics_dabs(h.0, 0, bad.as_ptr(), 11, out.as_mut_ptr(), 64),
            0
        );
        let bad = sample(1.0, 1.0, f64::INFINITY, 1.0, (0.0, 0.0), (1.0, 0.0));
        assert_eq!(
            dr_brush_dynamics_dabs(h.0, 0, bad.as_ptr(), 11, out.as_mut_ptr(), 64),
            0
        );
        assert!(out.iter().all(|&v| v == -7.0));
        assert_eq!(
            dr_brush_dynamics_dabs(h.0, 0, s.as_ptr(), 11, out.as_mut_ptr(), 18),
            3
        );
    }
}
