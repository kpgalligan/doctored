//! Brush dynamics' dual brush (`dr_brush_dual_create`, `dr_brush_dual_walk`,
//! `dr_brush_u16_dual`, `dr_brush_overlay_rgba8_dual`; `brush_dual`,
//! `brush_coverage` THE DUAL STROKE), black-box through the C ABI against an
//! oracle written here from the rule alone: its own SplitMix64 over the
//! documented positional seed `2³² | seed`, its own walk (one stamp at the
//! first point, then a stamp every `max(1, size · spacing / 100)` toward each
//! later point, the direction the move from the anchor before the step), its
//! own scatter, and its own coverage — each dab's rotated ellipse (soft: the
//! smoothstep at the pixel centre; hard: the mean of 8×8 samples), times its
//! deposit and, on the first tip, the texture factor, accumulated source-over
//! in dab order per list — then the two fields' f64 product and one rounding
//! at the first request's opacity.
use crate::native_scalar::{error, text, LIMIT};
use doctored_core::ffi_brush::*;
use doctored_core::ffi_brush_dual::*;
use doctored_core::ffi_brush_dynamics::*;
use doctored_core::ffi_brush_texture::*;
use serde_json::{json, Value};
use std::ptr;

const W: u32 = 48;
const H: u32 = 36;

const RANGE: &str = "native brush dual holds a size from 1 to 5000 and a spacing from 1 to 200";
const STAMPED: &str = "native brush dual requires stamped tips that are not aliased";
const WORK: &str =
    "native brush exceeds 2 billion footprint work units; use a shorter stroke or a smaller brush";

/// The dual handle, freed on drop.
struct Dual(*mut DrBrushDual);

impl Drop for Dual {
    fn drop(&mut self) {
        unsafe { dr_brush_dual_free(self.0) }
    }
}

fn create(request: Value) -> Result<Dual, String> {
    let t = text(request);
    let mut e = ptr::null_mut();
    let d = unsafe { dr_brush_dual_create(t.as_ptr(), &mut e) };
    if d.is_null() {
        Err(unsafe { error(e) })
    } else {
        assert!(e.is_null());
        Ok(Dual(d))
    }
}

fn dual(seed: u32, size: f64, spacing: f64, scatter: Value) -> Dual {
    create(json!({"version":1,"seed":seed,"size":size,"spacing":spacing,"scatter":scatter}))
        .unwrap()
}

/// One `dr_brush_dual_walk` call: the dabs, the anchor and the stamps, or
/// None for a refusal (which must leave both buffers untouched).
fn walk_once(
    d: &Dual,
    index: u64,
    anchor: Option<[f64; 2]>,
    to: [f64; 2],
    cap: usize,
) -> Option<(Vec<[f64; 4]>, [f64; 2], u64)> {
    let mut out = vec![-7.; cap + 8];
    let mut step = [-7.; 3];
    let n = unsafe {
        dr_brush_dual_walk(
            d.0,
            index,
            anchor.as_ref().map_or(ptr::null(), |a| a.as_ptr()),
            to[0],
            to[1],
            out.as_mut_ptr(),
            cap,
            step.as_mut_ptr(),
        )
    };
    if step == [-7.; 3] {
        assert_eq!(n, 0);
        assert!(out.iter().all(|&v| v == -7.), "a refusal wrote dabs");
        return None;
    }
    assert!(
        out[4 * n..].iter().all(|&v| v == -7.),
        "wrote past its dabs"
    );
    assert!(4 * n <= cap);
    let dabs = out[..4 * n]
        .chunks(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    Some((dabs, [step[0], step[1]], step[2] as u64))
}

/// Feeds every point, calling again with the same point until a call lays
/// no stamp (the capacity continuation); the dabs and the stamps laid.
fn walk_all(d: &Dual, points: &[[f64; 2]], cap: usize) -> (Vec<[f64; 4]>, u64) {
    let (mut dabs, mut index, mut anchor) = (Vec::new(), 0_u64, None);
    for &p in points {
        loop {
            let (laid, at, stamps) = walk_once(d, index, anchor, p, cap).expect("refused");
            dabs.extend(laid);
            anchor = Some(at);
            index += stamps;
            if stamps == 0 {
                break;
            }
        }
    }
    (dabs, index)
}

// ---- the oracle ---------------------------------------------------------

/// SplitMix64 at the positional seed `seed ^ a·γ ^ b·m1 ^ c·m2`, unit values
/// from the top 53 bits.
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

#[derive(Clone, Copy)]
struct Scatter {
    amount: f64,
    both: bool,
    count: u32,
    count_jitter: f64,
}

const NONE: Scatter = Scatter {
    amount: 0.,
    both: false,
    count: 1,
    count_jitter: 0.,
};

impl Scatter {
    fn json(self) -> Value {
        json!({"amount":self.amount,"both_axes":self.both,"count":self.count,
               "count_jitter":self.count_jitter})
    }
}

/// The rule's stamps: one at the first point, then a stamp `step` along each
/// move from the anchor while the point is at least `step` away; each with
/// the direction from the anchor before the step (zero for the first).
fn oracle_stamps(points: &[[f64; 2]], size: f64, spacing: f64) -> Vec<([f64; 2], [f64; 2])> {
    let step = (size * spacing / 100.).max(1.);
    let mut stamps = Vec::new();
    let mut anchor: Option<[f64; 2]> = None;
    for &p in points {
        let Some(mut a) = anchor else {
            stamps.push((p, [0., 0.]));
            anchor = Some(p);
            continue;
        };
        loop {
            let (dx, dy) = (p[0] - a[0], p[1] - a[1]);
            let length = dx.hypot(dy);
            if length < step {
                break;
            }
            a = [a[0] + dx / length * step, a[1] + dy / length * step];
            stamps.push((a, [dx, dy]));
        }
        anchor = Some(a);
    }
    stamps
}

/// Second stamp `j` at `at` resolved into its dabs: the count (jittered by
/// the first draw of stream (j, 0, 1)), each dab `k`'s offset from draws 3
/// and 4 of stream (j, k, 2) across the unit direction (and along it with
/// both axes), or at the angle 2π·u4 with no direction.
fn oracle_dabs(
    seed: u32,
    j: u64,
    at: [f64; 2],
    direction: [f64; 2],
    size: f64,
    s: Scatter,
) -> Vec<[f64; 4]> {
    let seed = (1_u64 << 32) | u64::from(seed);
    let n = if s.count_jitter > 0. {
        let u = Draws::at(seed, j, 0, 1).unit();
        ((f64::from(s.count) * (1. - s.count_jitter * u)).round() as u32).max(1)
    } else {
        s.count
    };
    let length = direction[0].hypot(direction[1]);
    (0..n)
        .map(|k| {
            let mut u = Draws::at(seed, j, u64::from(k), 2);
            let (_, _, u3, u4) = (u.unit(), u.unit(), u.unit(), u.unit());
            let (mut x, mut y) = (at[0], at[1]);
            if s.amount > 0. {
                let across = s.amount * size * (2. * u3 - 1.);
                if length > 0. {
                    let (dx, dy) = (direction[0] / length, direction[1] / length);
                    x += -dy * across;
                    y += dx * across;
                    if s.both {
                        let along = s.amount * size * (2. * u4 - 1.);
                        x += dx * along;
                        y += dy * along;
                    }
                } else {
                    let a = std::f64::consts::TAU * u4;
                    x += a.cos() * across;
                    y += a.sin() * across;
                }
            }
            [x, y, size, 1.]
        })
        .collect()
}

fn oracle_walk(
    seed: u32,
    points: &[[f64; 2]],
    size: f64,
    spacing: f64,
    s: Scatter,
) -> Vec<[f64; 4]> {
    oracle_stamps(points, size, spacing)
        .into_iter()
        .enumerate()
        .flat_map(|(j, (at, dir))| oracle_dabs(seed, j as u64, at, dir, size, s))
        .collect()
}

fn close(got: &[[f64; 4]], want: &[[f64; 4]], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: dab count");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g[0] - w[0]).abs() < 1e-9 && (g[1] - w[1]).abs() < 1e-9,
            "{what} dab {i}: {g:?} against {w:?}"
        );
        assert_eq!((g[2], g[3]), (w[2], w[3]), "{what} dab {i}");
    }
}

/// One dab's coverage of pixel (x, y) at `angle` and `roundness`, before its
/// deposit (the ellipse's normalized distance; soft: smoothstep at the pixel
/// centre; hard: the mean of 8×8 samples inside).
fn oracle_pixel(dab: &[f64], hardness: f64, angle: f64, roundness: f64, x: u32, y: u32) -> f64 {
    let (cx, cy, r) = (dab[0], dab[1], dab[2] / 2.);
    let (s, c) = angle.to_radians().sin_cos();
    let distance = |px: f64, py: f64| {
        let (dx, dy) = (px - cx, py - cy);
        let (u, v) = (c * dx - s * dy, s * dx + c * dy);
        (u / r).hypot(v / (r * roundness))
    };
    if hardness < 1. {
        let t = distance(f64::from(x) + 0.5, f64::from(y) + 0.5);
        if t <= hardness {
            1.
        } else if t >= 1. {
            0.
        } else {
            let k = (t - hardness) / (1. - hardness);
            1. - k * k * (3. - 2. * k)
        }
    } else {
        let mut inside = 0;
        for sy in 0..8 {
            for sx in 0..8 {
                let px = f64::from(x) + f64::from(2 * sx + 1) / 16.;
                let py = f64::from(y) + f64::from(2 * sy + 1) / 16.;
                if distance(px, py) <= 1. {
                    inside += 1;
                }
            }
        }
        f64::from(inside) / 64.
    }
}

/// One list's field: each dab at its shape (angle 0, roundness 1 without
/// one), times its deposit and `factor`, source-over in order.
fn oracle_field(
    hardness: f64,
    dabs: &[f64],
    shapes: Option<&[f64]>,
    factor: &dyn Fn(i64, i64) -> f64,
) -> Vec<f64> {
    let mut field = vec![0_f64; (W * H) as usize];
    for (i, dab) in dabs.chunks_exact(4).enumerate() {
        let (angle, roundness) = shapes.map_or((0., 1.), |s| (s[2 * i], s[2 * i + 1]));
        for y in 0..H {
            for x in 0..W {
                let c = oracle_pixel(dab, hardness, angle, roundness, x, y)
                    * dab[3]
                    * factor(i64::from(x), i64::from(y));
                let old = &mut field[(y * W + x) as usize];
                *old += c * (1. - *old);
            }
        }
    }
    field
}

/// The rule's output: the product of the two fields, one rounding at the
/// first request's opacity.
fn oracle(first: &[f64], second: &[f64], opacity: f64) -> Vec<u16> {
    first
        .iter()
        .zip(second)
        .map(|(a, b)| (a * b * opacity * 65535.).round() as u16)
        .collect()
}

fn unit(_: i64, _: i64) -> f64 {
    1.
}

/// The pixels where `got` and `want` differ by more than one code.
fn misses(got: &[u16], want: &[u16]) -> usize {
    got.iter()
        .zip(want)
        .filter(|(a, b)| a.abs_diff(**b) > 1)
        .count()
}

// ---- the rasterizers ------------------------------------------------------

fn request(hardness: f64, opacity: f64) -> Value {
    json!({"version":1,"hardness":hardness,"roundness":1.,"angle_degrees":0.,"pixel":false,
           "opacity":opacity})
}

fn plain(request: &Value, dabs: &[f64]) -> Vec<u16> {
    let t = text(request.clone());
    let mut out = vec![777; (W * H) as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16(
            W,
            H,
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
    assert!(ok, "{}", unsafe { error(e) });
    out
}

/// The dual stroke's inputs past the first request and list.
struct Second<'a> {
    request: Value,
    dabs: &'a [f64],
}

/// `dr_brush_u16_dual` on a `w` × `h` canvas, or its refusal; a refusal must
/// leave the output untouched.
#[allow(clippy::too_many_arguments)]
fn u16_dual_at(
    (w, h): (u32, u32),
    first: &Value,
    dabs: &[f64],
    shapes: Option<&[f64]>,
    texture: *const DrBrushTexture,
    second: Option<&Second>,
    out_len: usize,
) -> Result<Vec<u16>, String> {
    let t = text(first.clone());
    let s = second.map(|s| text(s.request.clone()));
    let mut out = vec![777; out_len];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16_dual(
            w,
            h,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            shapes.map_or(ptr::null(), <[f64]>::as_ptr),
            texture,
            0,
            0,
            s.as_ref().map_or(ptr::null(), |s| s.as_ptr()),
            second.map_or(ptr::null(), |s| s.dabs.as_ptr()),
            second.map_or(0, |s| s.dabs.len() / 4),
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
        assert!(out.iter().all(|&v| v == 777), "a refusal wrote output");
        Err(unsafe { error(e) })
    }
}

fn u16_dual(
    first: &Value,
    dabs: &[f64],
    shapes: Option<&[f64]>,
    texture: *const DrBrushTexture,
    second: &Second,
) -> Result<Vec<u16>, String> {
    u16_dual_at(
        (W, H),
        first,
        dabs,
        shapes,
        texture,
        Some(second),
        (W * H) as usize,
    )
}

/// `dr_brush_overlay_rgba8_dual` over the whole canvas, or its refusal.
fn overlay_dual(
    first: &Value,
    dabs: &[f64],
    second: &Second,
    ink: [u8; 3],
    selection: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let t = text(first.clone());
    let s = text(second.request.clone());
    let mut out = vec![77; (W * H * 4) as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_overlay_rgba8_dual(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            ptr::null(),
            ptr::null(),
            0,
            0,
            s.as_ptr(),
            second.dabs.as_ptr(),
            second.dabs.len() / 4,
            ink.as_ptr(),
            selection.map_or(ptr::null(), <[u8]>::as_ptr),
            selection.map_or(0, <[u8]>::len),
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
        assert!(out.iter().all(|&v| v == 77), "a refusal wrote output");
        Err(unsafe { error(e) })
    }
}

const FIRST: [f64; 16] = [
    12.3, 14.7, 15., 1., //
    18.8, 16.2, 12.5, 0.65, //
    26.4, 18.9, 17., 0.9, //
    33.1, 20.6, 11.25, 1.,
];

const SECOND: [f64; 24] = [
    11.6, 13.1, 6., 1., //
    15.2, 15.8, 7.5, 1., //
    19.9, 17.4, 5., 1., //
    24.3, 16.6, 8., 1., //
    29.7, 20.2, 6.5, 1., //
    35.4, 21.9, 7., 1.,
];

// ---- the walk -------------------------------------------------------------

#[test]
fn the_walk_lays_a_stamp_every_spacing_along_the_path() {
    let points = [
        [5.25, 7.5],
        [41.0, 9.75],
        [44.5, 30.0],
        [44.75, 30.5],
        [12.0, 28.125],
    ];
    for (size, spacing) in [(10., 25.), (10., 200.), (3., 20.), (37., 60.)] {
        let d = dual(3, size, spacing, NONE.json());
        let (got, stamps) = walk_all(&d, &points, 4096);
        let want = oracle_walk(3, &points, size, spacing, NONE);
        close(&got, &want, &format!("size {size} spacing {spacing}"));
        assert_eq!(stamps as usize, want.len());
        // Consecutive stamps along one move sit exactly one step apart.
        let step = (size * spacing / 100.).max(1.);
        for pair in got.windows(2).take(3) {
            let gap = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
            assert!((gap - step).abs() < 1e-9, "gap {gap} step {step}");
        }
    }
    // The spacing matters: 25 % and 200 % lay different stamps.
    let tight = walk_all(&dual(3, 10., 25., NONE.json()), &points, 4096).0;
    let wide = walk_all(&dual(3, 10., 200., NONE.json()), &points, 4096).0;
    assert!(
        tight.len() > 4 * wide.len(),
        "{} {}",
        tight.len(),
        wide.len()
    );
}

#[test]
fn one_long_move_lays_what_many_short_collinear_moves_lay() {
    let s = Scatter {
        amount: 1.5,
        both: true,
        count: 3,
        count_jitter: 0.5,
    };
    let (a, b) = ([3.0, 4.0], [83.0, 64.0]);
    let d = dual(17, 6., 35., s.json());
    let long = walk_all(&d, &[a, b], 4096).0;
    let short: Vec<[f64; 2]> = (0..=37)
        .map(|k| {
            let t = f64::from(k) / 37.;
            [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
        })
        .collect();
    let split = walk_all(&d, &short, 4096).0;
    close(&split, &long, "split");
    close(&long, &oracle_walk(17, &[a, b], 6., 35., s), "oracle");
    assert!(long.len() > 40);
}

#[test]
fn a_walk_cut_short_by_its_capacity_continues_bit_for_bit() {
    let s = Scatter {
        amount: 2.,
        both: false,
        count: 4,
        count_jitter: 0.75,
    };
    let points = [[2.5, 3.5], [70.25, 41.0], [10.0, 60.0]];
    let d = dual(99, 4., 50., s.json());
    let whole = walk_all(&d, &points, 1 << 16);
    for cap in [16, 17, 40, 63] {
        let cut = walk_all(&d, &points, cap);
        assert_eq!(cut.1, whole.1, "cap {cap}");
        assert_eq!(cut.0.len(), whole.0.len(), "cap {cap}");
        for (g, w) in cut.0.iter().zip(&whole.0) {
            assert_eq!(g.map(f64::to_bits), w.map(f64::to_bits), "cap {cap}");
        }
    }
    // One stamp's worth of capacity lays exactly one stamp a call.
    let first = walk_once(&d, 0, None, points[0], 16).unwrap();
    assert_eq!((first.1, first.2), (points[0], 1));
    let next = walk_once(&d, 1, Some(first.1), points[1], 16).unwrap();
    assert_eq!(next.2, 1);
    let gap = (next.1[0] - points[0][0]).hypot(next.1[1] - points[0][1]);
    assert!((gap - 2.).abs() < 1e-12, "{gap}");
    // A point within one step of the anchor lays nothing and keeps it.
    let still = walk_once(&d, 5, Some([10., 10.]), [10.5, 11.], 64).unwrap();
    assert_eq!(still, (vec![], [10., 10.], 0));
}

#[test]
fn the_second_stream_is_seeded_apart_from_the_first_tips() {
    let s = Scatter {
        amount: 2.,
        both: true,
        count: 2,
        count_jitter: 0.,
    };
    let d = dual(7, 12., 50., s.json());
    let got = walk_all(&d, &[[20., 20.], [60., 50.]], 4096).0;
    close(
        &got,
        &oracle_walk(7, &[[20., 20.], [60., 50.]], 12., 50., s),
        "stream",
    );
    // The first tip's own stream at seed 7, the same stamp and sample.
    let t = text(json!({"version":1,"seed":7,"scatter":s.json()}));
    let mut e = ptr::null_mut();
    let first = unsafe { dr_brush_dynamics_create(t.as_ptr(), &mut e) };
    assert!(!first.is_null());
    let nan = f64::NAN;
    let sample = [20., 20., 12., nan, nan, nan, 0., 0., nan, 0., 0.];
    let mut out = [0.; 12];
    let n = unsafe {
        dr_brush_dynamics_dabs(first, 0, sample.as_ptr(), 11, out.as_mut_ptr(), out.len())
    };
    unsafe { dr_brush_dynamics_free(first) };
    assert_eq!(n, 2);
    for k in 0..2 {
        let mine = [out[6 * k], out[6 * k + 1]];
        assert!(
            (mine[0] - got[k][0]).abs() > 1e-3 || (mine[1] - got[k][1]).abs() > 1e-3,
            "dab {k} shares the first tip's stream"
        );
    }
    // And the seed matters to the second stream.
    let other = walk_all(
        &dual(8, 12., 50., s.json()),
        &[[20., 20.], [60., 50.]],
        4096,
    )
    .0;
    assert_ne!(other[0][..2], got[0][..2]);
}

#[test]
fn dual_requests_and_walks_refuse_by_name_and_write_nothing() {
    for (size, spacing) in [
        (0.5, 25.),
        (5000.5, 25.),
        (-10., 25.),
        (10., 0.5),
        (10., 200.5),
    ] {
        let e = create(json!({"version":1,"size":size,"spacing":spacing}))
            .err()
            .unwrap_or_else(|| panic!("{size} {spacing} accepted"));
        assert_eq!(e, RANGE, "{size} {spacing}");
    }
    let cases = [
        (json!({"version":2,"size":10,"spacing":25}), "version 1"),
        (
            json!({"version":1,"spacing":25}),
            "invalid native brush dual",
        ),
        (
            json!({"version":1,"size":10,"spacing":25,"hardness":1}),
            "invalid native brush dual",
        ),
        (
            json!({"version":1,"size":10,"spacing":25,"scatter":{"amount":10.5}}),
            "scatter amount",
        ),
        (
            json!({"version":1,"size":10,"spacing":25,"scatter":{"count":17}}),
            "scatter count",
        ),
        (
            json!({"version":1,"size":10,"spacing":25,"scatter":{"count":0}}),
            "scatter count",
        ),
        (
            json!({"version":1,"size":10,"spacing":25,"scatter":{"count_jitter":1.5}}),
            "count_jitter",
        ),
        (
            json!({"version":1,"size":10,"spacing":25,"seed":-1}),
            "invalid native brush dual",
        ),
        (
            json!({"version":1,"size":10,"spacing":25,"pad":"x".repeat(1100)}),
            "exceeds",
        ),
    ];
    for (request, needle) in cases {
        let e = create(request.clone())
            .err()
            .unwrap_or_else(|| panic!("{request} was accepted"));
        assert!(e.contains(needle), "{request}: {e}");
    }
    // The ends of every range are accepted.
    for (size, spacing) in [(1., 1.), (5000., 200.)] {
        assert!(create(json!({"version":1,"size":size,"spacing":spacing})).is_ok());
    }
    let d = dual(0, 10., 25., json!({"count":3}));
    let nan = f64::NAN;
    assert!(
        walk_once(&d, 0, None, [1., 1.], 11).is_none(),
        "below one stamp"
    );
    assert!(walk_once(&d, 0, None, [nan, 1.], 64).is_none());
    assert!(walk_once(&d, 0, Some([nan, 0.]), [1., 1.], 64).is_none());
    assert!(walk_once(&d, 0, Some([0., 0.]), [1., f64::INFINITY], 64).is_none());
    assert!(walk_once(&d, 0, None, [1., 1.], 12).is_some());
    let mut out = [-7.; 16];
    let mut step = [-7.; 3];
    unsafe {
        assert_eq!(
            dr_brush_dual_walk(
                d.0,
                0,
                ptr::null(),
                1.,
                1.,
                ptr::null_mut(),
                16,
                step.as_mut_ptr()
            ),
            0
        );
        assert_eq!(
            dr_brush_dual_walk(
                d.0,
                0,
                ptr::null(),
                1.,
                1.,
                out.as_mut_ptr(),
                16,
                ptr::null_mut()
            ),
            0
        );
    }
    assert!(out.iter().chain(&step).all(|&v| v == -7.));
}

// ---- the coverage ---------------------------------------------------------

#[test]
fn two_soft_tips_multiply_their_fields_before_the_one_rounding() {
    for (h1, h2) in [(0.3, 0.5), (0.7, 0.)] {
        for opacity in [1., 0.6] {
            let first = request(h1, opacity);
            let second = Second {
                request: request(h2, 1.),
                dabs: &SECOND,
            };
            let got = u16_dual(&first, &FIRST, None, ptr::null(), &second).unwrap();
            let f1 = oracle_field(h1, &FIRST, None, &unit);
            let f2 = oracle_field(h2, &SECOND, None, &unit);
            let want = oracle(&f1, &f2, opacity);
            assert_eq!(misses(&got, &want), 0, "{h1} {h2} {opacity}");
            // Neither tip alone: the product thins both.
            assert_ne!(got, plain(&first, &FIRST));
            // Rounding each field first is off by more than a code somewhere.
            let twice: Vec<u16> = f1
                .iter()
                .zip(&f2)
                .map(|(a, b)| {
                    let q = |v: f64| (v * opacity * 65535.).round();
                    (q(*a) * q(*b) / 65535.).round() as u16
                })
                .collect();
            if opacity < 1. {
                assert!(misses(&twice, &want) > 50, "{h1} {h2}: twice is close");
            }
        }
    }
}

#[test]
fn the_second_requests_opacity_is_not_read_and_an_empty_second_list_paints_nothing() {
    let first = request(0.4, 0.8);
    let one = Second {
        request: request(0.6, 1.),
        dabs: &SECOND,
    };
    let thin = Second {
        request: request(0.6, 0.25),
        dabs: &SECOND,
    };
    assert_eq!(
        u16_dual(&first, &FIRST, None, ptr::null(), &one).unwrap(),
        u16_dual(&first, &FIRST, None, ptr::null(), &thin).unwrap()
    );
    let empty = Second {
        request: request(0.6, 1.),
        dabs: &[],
    };
    let got = u16_dual(&first, &FIRST, None, ptr::null(), &empty).unwrap();
    assert!(got.iter().all(|&v| v == 0));
}

#[test]
fn hard_interiors_are_exact_and_nothing_lies_outside_either_footprint() {
    // A hard 30 px first dab and a hard 10 px second dab inside it.
    let first = [24.5, 18.5, 30., 1.];
    let second = [24.25, 18.75, 10., 1.];
    let r = request(1., 1.);
    let got = u16_dual(
        &r,
        &first,
        None,
        ptr::null(),
        &Second {
            request: r.clone(),
            dabs: &second,
        },
    )
    .unwrap();
    let p1 = plain(&r, &first);
    let p2 = plain(&r, &second);
    let (mut full, mut thinned) = (0, 0);
    for i in 0..got.len() {
        if p1[i] == 65535 {
            // F1 is exactly 1: the product is the second tip's field.
            assert_eq!(got[i], p2[i], "pixel {i}");
            full += usize::from(p2[i] == 65535);
        }
        if p2[i] == 65535 {
            assert_eq!(got[i], p1[i], "pixel {i}");
        }
        if p2[i] == 0 {
            assert_eq!(got[i], 0, "pixel {i} lies outside the second footprint");
            thinned += usize::from(p1[i] > 0);
        }
    }
    assert!(full > 40 && thinned > 300, "{full} {thinned}");
    let want = oracle(
        &oracle_field(1., &first, None, &unit),
        &oracle_field(1., &second, None, &unit),
        1.,
    );
    assert_eq!(misses(&got, &want), 0);
}

#[test]
fn a_hard_one_pixel_second_tip_is_the_classified_dab_not_the_pixel_brush() {
    let first = [24., 18., 34., 1.];
    let second: Vec<f64> = (0..9)
        .flat_map(|k| {
            let k = f64::from(k);
            [10.3 + 3.1 * k, 9.6 + 2.2 * k, 1., 1.]
        })
        .collect();
    for h1 in [0.5, 1.] {
        let r = request(h1, 1.);
        let got = u16_dual(
            &r,
            &first,
            None,
            ptr::null(),
            &Second {
                request: request(1., 1.),
                dabs: &second,
            },
        )
        .unwrap();
        let want = oracle(
            &oracle_field(h1, &first, None, &unit),
            &oracle_field(1., &second, None, &unit),
            1.,
        );
        assert_eq!(misses(&got, &want), 0, "{h1}");
        assert!(got.iter().filter(|&&v| v > 0).count() >= 9);
        // A 1 px hard dab at a fractional centre is partial, never a full pixel.
        assert!(got.iter().all(|&v| v < 65535));
    }
}

/// A 5 × 3 tile of greys and part-transparent texels.
fn tile() -> (u32, u32, Vec<u8>) {
    let texels: [[u8; 4]; 15] = [
        [0, 0, 0, 255],
        [255, 255, 255, 255],
        [128, 128, 128, 255],
        [255, 0, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 0],
        [255, 255, 255, 128],
        [64, 200, 30, 255],
        [255, 255, 255, 255],
        [10, 10, 10, 255],
        [200, 200, 200, 255],
        [0, 255, 0, 255],
        [90, 90, 90, 64],
        [255, 255, 255, 255],
        [30, 60, 90, 255],
    ];
    (5, 3, texels.concat())
}

#[test]
fn the_first_tips_shapes_and_texture_carry_into_the_product() {
    let (tw, th, rgba) = tile();
    let depth = 0.8;
    let mut e = ptr::null_mut();
    let texture = unsafe { dr_brush_texture_create(rgba.as_ptr(), tw, th, 1., depth, &mut e) };
    assert!(!texture.is_null());
    // Scale 1 reads texel (x mod w, y mod h); t is the luminosity of the
    // premultiplied colour.
    let factor = |x: i64, y: i64| {
        let k = ((y.rem_euclid(3) * 5 + x.rem_euclid(5)) * 4) as usize;
        let a = f64::from(rgba[k + 3]) / 255.;
        let c = |n: usize| f64::from(rgba[k + n]) / 255. * a;
        let t = 0.30 * c(0) + 0.59 * c(1) + 0.11 * c(2);
        1. - depth * (1. - t)
    };
    let shapes = [30., 0.4, -75., 0.9, 180., 0.2, 111.5, 0.6];
    for (h1, h2) in [(1., 0.5), (0.4, 1.)] {
        let r = request(h1, 1.);
        let second = Second {
            request: request(h2, 1.),
            dabs: &SECOND,
        };
        let got = u16_dual(&r, &FIRST, Some(&shapes), texture, &second).unwrap();
        let want = oracle(
            &oracle_field(h1, &FIRST, Some(&shapes), &factor),
            &oracle_field(h2, &SECOND, None, &unit),
            1.,
        );
        assert_eq!(misses(&got, &want), 0, "{h1} {h2}");
        let plain_dual = u16_dual(&r, &FIRST, None, ptr::null(), &second).unwrap();
        assert_ne!(got, plain_dual);
    }
    let r = request(0.5, 1.);
    let bad = [10., 0.5, 10., 0.005, 10., 0.5, 10., 0.5];
    assert_eq!(
        u16_dual(
            &r,
            &FIRST,
            Some(&bad),
            texture,
            &Second {
                request: request(0.5, 1.),
                dabs: &SECOND
            }
        )
        .unwrap_err(),
        "native brush shapes hold an angle from -180 to 180 and a roundness from 0.01 to 1 per dab"
    );
    unsafe { dr_brush_texture_free(texture) };
}

#[test]
fn the_dual_overlay_is_the_overlay_rule_over_the_dual_coverage() {
    let r = request(0.7, 0.9);
    let second = Second {
        request: request(0.4, 1.),
        dabs: &SECOND,
    };
    let coverage = u16_dual(&r, &FIRST, None, ptr::null(), &second).unwrap();
    let ink = [200, 40, 91];
    let selection: Vec<u8> = (0..W * H).map(|i| ((i * 37) % 256) as u8).collect();
    let round = |x: u32, d: u32| (2 * x + d) / (2 * d);
    for mask in [None, Some(selection.as_slice())] {
        let got = overlay_dual(&r, &FIRST, &second, ink, mask).unwrap();
        for (i, &c) in coverage.iter().enumerate() {
            let mut a = round(u32::from(c) * 255, 65535);
            if let Some(m) = mask {
                a = round(a * u32::from(m[i]), 255);
            }
            let want: Vec<u8> = ink
                .iter()
                .map(|&v| round(u32::from(v) * a, 255) as u8)
                .chain([a as u8])
                .collect();
            assert_eq!(&got[i * 4..i * 4 + 4], want.as_slice(), "pixel {i}");
        }
    }
}

#[test]
fn dual_rasterizer_refusals_name_their_rule_and_leave_the_output_untouched() {
    let soft = request(0.5, 1.);
    let pixel = json!({"version":1,"hardness":1.,"roundness":1.,"angle_degrees":0.,"pixel":true});
    let aliased = json!({"version":1,"hardness":1.,"roundness":1.,"angle_degrees":0.,
                         "pixel":false,"aliased":true});
    for tip in [pixel, aliased] {
        let on_second = Second {
            request: tip.clone(),
            dabs: &SECOND,
        };
        let plain_second = Second {
            request: soft.clone(),
            dabs: &SECOND,
        };
        assert_eq!(
            u16_dual(&soft, &FIRST, None, ptr::null(), &on_second).unwrap_err(),
            STAMPED
        );
        assert_eq!(
            u16_dual(&tip, &FIRST, None, ptr::null(), &plain_second).unwrap_err(),
            STAMPED
        );
        assert_eq!(
            overlay_dual(&soft, &FIRST, &on_second, [1, 2, 3], None).unwrap_err(),
            STAMPED
        );
    }
    // A NULL second request refuses as dr_brush_u16 refuses a NULL request.
    let e = u16_dual_at(
        (W, H),
        &soft,
        &FIRST,
        None,
        ptr::null(),
        None,
        (W * H) as usize,
    )
    .unwrap_err();
    assert!(e.contains("native brush request is NULL"), "{e}");
    // Each list's own dab cap.
    let many = vec![1.; 4 * 50_001];
    let over = Second {
        request: soft.clone(),
        dabs: &many,
    };
    let e = u16_dual(&soft, &FIRST, None, ptr::null(), &over).unwrap_err();
    assert!(e.contains("at most 50000 dabs"), "{e}");
    // The output length is checked against the canvas.
    let second = Second {
        request: soft.clone(),
        dabs: &SECOND,
    };
    let e = u16_dual_at((W, H), &soft, &FIRST, None, ptr::null(), Some(&second), 3).unwrap_err();
    assert!(e.contains("output length"), "{e}");
    // The two plans' work sums: each list alone fits the cap, together they
    // do not. Nine 10,000 px soft dabs on a 10,000² canvas plan 1.1·10⁹ each.
    let big: Vec<f64> = (0..9).flat_map(|_| [5000., 5000., 10000., 1.]).collect();
    let side = 10_000;
    let alone = Second {
        request: soft.clone(),
        dabs: &[],
    };
    let both = Second {
        request: soft.clone(),
        dabs: &big,
    };
    let t = text(soft.clone());
    let plan = unsafe {
        dr_brush_u16_plan(
            side,
            side,
            t.as_ptr(),
            big.as_ptr(),
            9,
            LIMIT,
            0,
            ptr::null_mut(),
        )
    };
    assert!(!plan.is_null(), "one list alone plans");
    unsafe { doctored_core::ffi::dr_string_free(plan) };
    assert_eq!(
        u16_dual_at((side, side), &soft, &big, None, ptr::null(), Some(&both), 1).unwrap_err(),
        WORK
    );
    assert!(u16_dual_at(
        (side, side),
        &soft,
        &big,
        None,
        ptr::null(),
        Some(&alone),
        1
    )
    .unwrap_err()
    .contains("output length"));
}
