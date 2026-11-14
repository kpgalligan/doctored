//! Phase 13A-6b: native artwork coverage by an ACTIVE-EDGE SWEEP
//! (`artwork_coverage::rasterize`, reached through the unchanged
//! `dr_artwork_path_coverage`), whose row body is itself a y-ordered pair and
//! interval sweep. The oracle is the retained FULL SCAN,
//! `artwork_coverage::reference_rasterize` (the pre-13A-6b every-edge-per-row
//! scan over a row band, running the earlier all-pairs row body): within the
//! work cap the sweep equals it sample for sample; beyond it (the full scan
//! refuses with the work-limit sentence) the sweep finishes at the canvas
//! dimensions and equals the full scan concatenated over row bands that each
//! fit the cap — a tall dense field (13A-6b) and one long single line.
//! The row body integrates each x-cluster of closed loops on its
//! own breakpoints: a one-cluster row is still bit-identical, a row of several
//! may differ by one code (`assert_within_one`, the 1,000-glyph line).
//! The boundary fixture pins MONOTONICITY: an outline whose full-scan total
//! is exactly the cap is accepted by both. The pair stage charges a
//! pair of full edges only when the two cross (`artwork_pair_sweep`), so an
//! outline of parallel full edges past the reference's `active²` cap finishes
//! (zero area, and a hatch with a closed-form area), crossing full edges
//! equal the full scan, and an outline whose full edges all cross still
//! refuses. The pair stage polls every 4,096 pair tests.
//!
//! Default tests are load-independent (13A-5b-iii-b-1): a pre-cancelled
//! token, a cancel PARKED inside the row loop (13A-5b-iii-b-3's seam) and a
//! cancelled-then-complete run equal to the reference. DECLARED before
//! measuring (13A-5a's model `T_f + W_s·c_s`, `T_f` = 1 ms): a cancel issued
//! inside the row loop returns within `T_f + c_row`, `c_row` one row from the
//! uncancelled progress log; the `#[ignore]`d probe prints PASS/MISS through
//! `make cancel-bench`. Misses are recorded, never widened.

use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::artwork_coverage::reference_rasterize;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::dr_artwork_path_coverage;
use doctored_core::ffi_jobs::*;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

use crate::common;
use common::probe::*;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SENTENCE: &str =
    "artwork outline exceeds the bounded area-integration work limit; simplify the path or reduce its size";
const SENTINEL: u16 = 0x4a4a;

type Point = [f64; 2];

/// A path: verbs and points.
struct Path {
    commands: Vec<u8>,
    points: Vec<Point>,
}

impl Path {
    fn new() -> Self {
        Self {
            commands: Vec::new(),
            points: Vec::new(),
        }
    }

    /// One closed polygon subpath.
    fn polygon(mut self, points: &[Point]) -> Self {
        self.commands.push(0);
        self.commands
            .extend(std::iter::repeat_n(1, points.len() - 1));
        self.commands.push(4);
        self.points.extend_from_slice(points);
        self
    }

    /// `ring(cx, cy, r, 256)`: one 256-gon subpath — `move`, 255 `line`,
    /// `close` — whose 256 edges are all non-horizontal.
    fn ring(self, cx: f64, cy: f64, r: f64, sides: u32) -> Self {
        let points = (0..sides)
            .map(|k| {
                let t = std::f64::consts::TAU * f64::from(k) / f64::from(sides);
                [cx + r * t.cos(), cy + r * t.sin()]
            })
            .collect::<Vec<_>>();
        self.polygon(&points)
    }

    /// A circle-like ring of four cubics.
    fn cubic_ring(mut self, cx: f64, cy: f64, r: f64) -> Self {
        let k = 0.552_284_749_8 * r;
        self.commands.extend([0, 3, 3, 3, 3, 4]);
        self.points.extend([
            [cx + r, cy],
            [cx + r, cy + k],
            [cx + k, cy + r],
            [cx, cy + r],
            [cx - k, cy + r],
            [cx - r, cy + k],
            [cx - r, cy],
            [cx - r, cy - k],
            [cx - k, cy - r],
            [cx, cy - r],
            [cx + k, cy - r],
            [cx + r, cy - k],
            [cx + r, cy],
        ]);
        self
    }
}

fn rectangle(left: f64, top: f64, right: f64, bottom: f64) -> Vec<Point> {
    vec![[left, top], [right, top], [right, bottom], [left, bottom]]
}

fn rect_path(left: f64, top: f64, right: f64, bottom: f64) -> Path {
    Path::new().polygon(&rectangle(left, top, right, bottom))
}

// ------------------------------------------------------------- plumbing --

unsafe fn message(e: *mut c_char) -> String {
    if e.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(e) };
    s
}

/// `dr_artwork_path_coverage` into a `0x4a4a`-filled output under `control`
/// (NULL: none). On refusal the output must be untouched.
fn product_with(
    path: &Path,
    w: u32,
    h: u32,
    even_odd: bool,
    control: *const DrJobControl,
) -> Result<Vec<u16>, String> {
    let mut out = vec![SENTINEL; w as usize * h as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_artwork_path_coverage(
            path.commands.as_ptr(),
            path.commands.len(),
            path.points.as_ptr().cast(),
            path.points.len(),
            w,
            h,
            even_odd,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            control,
            &mut e,
        )
    };
    let m = unsafe { message(e) };
    if ok {
        Ok(out)
    } else {
        assert!(
            out.iter().all(|&v| v == SENTINEL),
            "a refused coverage wrote its output"
        );
        Err(m)
    }
}

fn product(path: &Path, w: u32, h: u32, even_odd: bool) -> Result<Vec<u16>, String> {
    product_with(path, w, h, even_odd, ptr::null())
}

fn reference(path: &Path, w: u32, h: u32, even_odd: bool) -> Result<Vec<u16>, String> {
    reference_rasterize(&path.commands, &path.points, w, h, even_odd, (0, h))
}

/// The full scan concatenated over `band`-row bands, each band's own call
/// asserted `Ok`.
fn banded_oracle(path: &Path, w: u32, h: u32, even_odd: bool, band: u32) -> Vec<u16> {
    let mut out = Vec::with_capacity(w as usize * h as usize);
    let mut start = 0;
    while start < h {
        let end = (start + band).min(h);
        let part = reference_rasterize(&path.commands, &path.points, w, h, even_odd, (start, end))
            .unwrap_or_else(|e| panic!("the band [{start}, {end}) refused: {e}"));
        assert_eq!(part.len(), w as usize * (end - start) as usize);
        out.extend_from_slice(&part);
        start = end;
    }
    out
}

fn first_difference(a: &[u16], b: &[u16], w: u32) -> String {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => format!(
            "first difference at ({}, {}): {} vs {}",
            i as u32 % w,
            i as u32 / w,
            a[i],
            b[i]
        ),
        None => format!("lengths {} vs {}", a.len(), b.len()),
    }
}

/// Within the cap: the sweep equals the full scan exactly.
fn assert_exact(path: &Path, w: u32, h: u32, even_odd: bool, what: &str) {
    let expected =
        reference(path, w, h, even_odd).unwrap_or_else(|e| panic!("{what}: reference: {e}"));
    let actual = product(path, w, h, even_odd).unwrap_or_else(|e| panic!("{what}: sweep: {e}"));
    assert!(
        actual == expected,
        "{what} (even_odd {even_odd}) differs from the full scan ({})",
        first_difference(&actual, &expected, w)
    );
}

/// The tolerance: a row with several x-clusters integrates each on its own
/// breakpoints, so it may differ from the full scan by at most one code.
fn assert_within_one(actual: &[u16], expected: &[u16], w: u32, what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: lengths");
    if let Some(i) = actual
        .iter()
        .zip(expected)
        .position(|(a, b)| a.abs_diff(*b) > 1)
    {
        panic!(
            "{what} differs by more than one code at ({}, {}): {} vs {}",
            i as u32 % w,
            i as u32 / w,
            actual[i],
            expected[i]
        );
    }
}

fn assert_exact_both(path: &Path, w: u32, h: u32, what: &str) {
    assert_exact(path, w, h, false, what);
    assert_exact(path, w, h, true, what);
}

// --------------------------------------------------------- within the cap --

#[test]
fn geometry_equals_the_full_scan() {
    const W: u32 = 24;
    const H: u32 = 12;
    for r in [
        (1.25, 2.5, 17.75, 9.125),
        (0.3, 0.7, 23.9, 11.2),
        (5.5, 5.25, 5.75, 5.5),
    ] {
        assert_exact_both(&rect_path(r.0, r.1, r.2, r.3), W, H, "fractional rectangle");
    }
    let slanted = Path::new().polygon(&[[-5.3, 2.2], [20.7, -3.1], [30.4, 14.9], [3.3, 18.6]]);
    assert_exact_both(&slanted, W, H, "slanted polygon partly off-canvas");
    // Outer and inner rectangles in the SAME direction: a hole under even-odd,
    // filled under nonzero; reversed inner: a hole under both.
    let same = Path::new()
        .polygon(&rectangle(1.5, 1.25, 22.5, 10.75))
        .polygon(&rectangle(6.25, 3.5, 15.75, 8.5));
    assert_exact_both(&same, W, H, "same-direction hole");
    let mut reversed = rectangle(6.25, 3.5, 15.75, 8.5);
    reversed.reverse();
    let reversed = Path::new()
        .polygon(&rectangle(1.5, 1.25, 22.5, 10.75))
        .polygon(&reversed);
    assert_exact_both(&reversed, W, H, "reversed hole");
    let star = Path::new().polygon(
        &(0..5)
            .map(|k| {
                let t = std::f64::consts::TAU * f64::from(k * 2) / 5.0 - 1.3;
                [12.1 + 5.7 * t.cos(), 6.2 + 5.3 * t.sin()]
            })
            .collect::<Vec<_>>(),
    );
    assert_exact_both(&star, W, H, "self-intersecting star");
    let bowtie = Path::new().polygon(&[[2.2, 1.1], [20.3, 10.4], [20.9, 1.7], [1.6, 9.8]]);
    assert_exact_both(&bowtie, W, H, "bowtie");
    let cubic = Path::new()
        .cubic_ring(11.3, 5.9, 5.2)
        .cubic_ring(11.3, 5.9, 2.4);
    assert_exact_both(&cubic, W, H, "4-cubic rings");
}

#[test]
fn canvas_edges_equal_the_full_scan() {
    const W: u32 = 24;
    const H: u32 = 12;
    let cases = [
        ("entirely above", rect_path(3.2, -9.7, 17.1, -2.3)),
        (
            "entirely above, touching row 0",
            rect_path(3.2, -9.7, 17.1, 0.0),
        ),
        ("entirely below", rect_path(3.2, 14.1, 17.1, 20.6)),
        (
            "entirely below, touching y == H",
            rect_path(3.2, 12.0, 17.1, 20.6),
        ),
        ("entirely left", rect_path(-9.2, 2.1, -0.4, 9.6)),
        ("entirely right", rect_path(24.5, 2.1, 31.4, 9.6)),
        ("straddling the top edge", rect_path(2.6, -3.3, 19.4, 4.7)),
        (
            "straddling the bottom edge",
            rect_path(2.6, 8.3, 19.4, 15.9),
        ),
        ("endpoints on integer rows", rect_path(2.0, 3.0, 10.0, 7.0)),
        ("bottom at y == H", rect_path(2.5, 5.0, 10.5, 12.0)),
        ("whole canvas", rect_path(0.0, 0.0, 24.0, 12.0)),
        (
            "slant through integer rows",
            Path::new().polygon(&[[1.0, 0.0], [9.0, 4.0], [3.0, 12.0]]),
        ),
        ("empty path", Path::new()),
        (
            "horizontal-only path",
            Path {
                commands: vec![0, 1, 1, 4],
                points: vec![[1.0, 2.0], [8.0, 2.0], [3.0, 2.0]],
            },
        ),
    ];
    for (what, path) in &cases {
        assert_exact_both(path, W, H, what);
    }
    for (what, path) in [
        ("1×1 covered", rect_path(-1.0, -1.0, 2.0, 2.0)),
        ("1×1 partly covered", rect_path(0.25, 0.5, 0.75, 3.0)),
        ("1×1 missed", rect_path(3.0, 3.0, 4.0, 4.0)),
    ] {
        assert_exact_both(&path, 1, 1, what);
    }
    // Zero-ink outlines finish as zeros at the canvas's dimensions.
    for (what, path) in &cases[..6] {
        let out = product(path, W, H, false).unwrap();
        assert!(out.iter().all(|&v| v == 0), "{what}");
    }
}

/// E = 51,200 (200 rings × 256 edges) over 1200×900: the full scan's total by
/// the charge formulas is ≈ 110M (`E·H` 46.1M + n 1.08M + body ≈ 63M), inside
/// the cap on BOTH paths — a real sample comparison.
#[test]
fn an_in_cap_glyph_field_equals_the_full_scan() {
    let mut path = Path::new();
    for j in 0..10 {
        for i in 0..20 {
            path = path.ring(f64::from(30 + 60 * i), f64::from(45 + 90 * j), 3.0, 256);
        }
    }
    assert_eq!(path.commands.len(), 200 * 257);
    assert_exact(&path, 1200, 900, false, "glyph field");
}

// --------------------------------------------------------- beyond the cap --

/// 1000 radius-3 rings (4 × 250 grid) on 400×4000: E = 256,000 ≤ 262,144,
/// 257,000 commands ≤ 262,144, `E·H` = 1.024·10⁹ > 256M. The full scan
/// refuses; the sweep (≈ 70M) finishes and equals the full scan over 200-row
/// bands (≈ 55M each).
#[test]
fn a_tall_dense_field_beyond_the_cap_finishes_and_equals_the_banded_full_scan() {
    const W: u32 = 400;
    const H: u32 = 4000;
    let mut path = Path::new();
    for j in 0..250 {
        for i in 0..4 {
            path = path.ring(f64::from(50 + 100 * i), f64::from(8 + 16 * j), 3.0, 256);
        }
    }
    assert_eq!(path.commands.len(), 257_000);
    assert_eq!(path.points.len(), 256_000);
    assert_eq!(
        reference(&path, W, H, false).err().as_deref(),
        Some(SENTENCE),
        "the fixture is not beyond the cap on the full scan"
    );
    let actual = product(&path, W, H, false).unwrap_or_else(|e| panic!("the sweep refused: {e}"));
    assert_eq!(actual.len(), (W * H) as usize);
    let expected = banded_oracle(&path, W, H, false, 200);
    assert!(
        actual == expected,
        "the sweep differs from the banded full scan ({})",
        first_difference(&actual, &expected, W)
    );
    assert!(actual.contains(&65535), "no ink");
}

/// One long single line — 200 radius-6 256-gon rings (a ~200-glyph
/// line), centres 13 px apart on one 14-row band, 2,614×14, E = 51,200.
/// Measured charges: the full scan's total is ≈ 288.2M (`E·H + n` 0.75M, then
/// per row `active²` plus `breaks·(active + 1)`, up to ≈ 85.5M in one row), so
/// it refuses; the pair sweep's was ≈ 28.4M, because a row's open and spanning
/// lists hold only the edges overlapping in y (≈ two per ring per interval),
/// not every active edge, and now each ring is its own x-cluster with
/// its own breakpoints, so it is lower still. Every 1-row full-scan band fits
/// (at most ≈ 85.5M), so the banded oracle is exact.
#[test]
fn a_long_single_line_beyond_the_old_row_charge_finishes_and_equals_the_banded_full_scan() {
    const N: u32 = 200;
    const W: u32 = 13 * N + 14;
    const H: u32 = 14;
    let mut path = Path::new();
    for i in 0..N {
        path = path.ring(f64::from(13 * i) + 7.3, 7.1, 6.0, 256);
    }
    assert_eq!(
        reference(&path, W, H, false).err().as_deref(),
        Some(SENTENCE),
        "the fixture is not beyond the cap on the full scan"
    );
    let actual = product(&path, W, H, false).unwrap_or_else(|e| panic!("the sweep refused: {e}"));
    assert_eq!(actual.len(), (W * H) as usize);
    let expected = banded_oracle(&path, W, H, false, 1);
    assert!(
        actual == expected,
        "the sweep differs from the banded full scan ({})",
        first_difference(&actual, &expected, W)
    );
    assert!(actual.contains(&65535), "no ink");
}

/// One 1,000-glyph line — 1,000 radius-6 256-gon rings, centres 13 px
/// apart on one 14-row band, 13,014×14, E = 256,000 (≤ `MAX_PATH_EDGES`).
/// The earlier sweep merged every ring's breakpoints into one row-wide list,
/// so a row's interval sweep visited ≈ 2·1,000 spanning edges in each of its
/// ≈ 80,000 intervals: its charge grew with the SQUARE of the line length
/// (≈ 28.4M at 200 rings, so ≈ 710M at 1,000) and exceeded `MAX_SCAN_WORK`
/// (256M), refusing with `SENTENCE` (measured before the change). Now
/// each ring is its own x-cluster and the line finishes. The oracle is
/// the one-ring tile: the full scan of one ring at the same fractional
/// centre on a 14×14 canvas, placed at each ring's integer column offset;
/// several clusters per row allow ≤ 1 code (`assert_within_one`).
#[test]
fn a_thousand_glyph_line_finishes_within_one_code_of_the_one_ring_tile() {
    const N: u32 = 1000;
    const W: u32 = 13 * N + 14;
    const H: u32 = 14;
    let mut path = Path::new();
    for i in 0..N {
        path = path.ring(f64::from(13 * i) + 7.3, 7.1, 6.0, 256);
    }
    assert_eq!(path.points.len(), 256_000);
    let actual = product(&path, W, H, false).unwrap_or_else(|e| panic!("the sweep refused: {e}"));
    let tile = reference(&Path::new().ring(7.3, 7.1, 6.0, 256), 14, H, false).unwrap();
    let mut expected = vec![0u16; (W * H) as usize];
    for i in 0..N as usize {
        for y in 0..H as usize {
            for c in 0..14 {
                let at = y * W as usize + 13 * i + c;
                expected[at] = expected[at].saturating_add(tile[y * 14 + c]);
            }
        }
    }
    assert_within_one(&actual, &expected, W, "the 1,000-glyph line");
    assert!(actual.contains(&65535), "no ink");
}

/// The module's flattening rule, restated as the oracle for an outline's
/// edge count: a cubic is one edge once both controls lie within 1/4096 px of
/// its chord (point-to-segment distance), else it splits at t = ½.
fn cubic_edges(p: [Point; 4]) -> usize {
    let off = |q: Point| {
        let (dx, dy) = (p[3][0] - p[0][0], p[3][1] - p[0][1]);
        let length = dx * dx + dy * dy;
        let t = if length == 0.0 {
            0.0
        } else {
            (((q[0] - p[0][0]) * dx + (q[1] - p[0][1]) * dy) / length).clamp(0.0, 1.0)
        };
        (q[0] - p[0][0] - t * dx).hypot(q[1] - p[0][1] - t * dy)
    };
    if off(p[1]).max(off(p[2])) <= 1.0 / 4096.0 {
        return 1;
    }
    let mid = |a: Point, b: Point| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let (a, b, c) = (mid(p[0], p[1]), mid(p[1], p[2]), mid(p[2], p[3]));
    let (d, e) = (mid(a, b), mid(b, c));
    let f = mid(d, e);
    cubic_edges([p[0], a, d, f]) + cubic_edges([f, e, c, p[3]])
}

/// One line of `n` radius-6 `cubic_ring`s, centres 13 px apart, and its
/// flattened edge count by `cubic_edges`.
fn cubic_line(n: u32) -> (Path, usize) {
    let mut path = Path::new();
    for i in 0..n {
        path = path.cubic_ring(f64::from(13 * i) + 7.3, 7.1, 6.0);
    }
    let edges = path
        .points
        .chunks(13)
        .map(|ring| {
            (0..4)
                .map(|k| {
                    cubic_edges([
                        ring[3 * k],
                        ring[3 * k + 1],
                        ring[3 * k + 2],
                        ring[3 * k + 3],
                    ])
                })
                .sum::<usize>()
        })
        .sum();
    (path, edges)
}

/// The flattened-edge bound is 524,288 (262,144 before), because a
/// curved glyph is hundreds of edges at the 1/4096 px flatness and the old
/// bound, not the scan work, was what stopped a long line. 1,000 four-cubic
/// rings on one 14-row band flatten to more than 262,144 edges (512 a ring by
/// `cubic_edges`) and finish; the oracle is the one-ring tile, as for the
/// 1,000-polygon line. 1,100 rings pass 524,288 with 14,300 points (far
/// inside `MAX_PATH_POINTS`) and refuse with the edge sentence, the output
/// untouched.
#[test]
fn a_line_past_the_old_edge_bound_finishes_and_one_past_the_bound_refuses() {
    const N: u32 = 1000;
    const W: u32 = 13 * N + 14;
    const H: u32 = 14;
    let (path, edges) = cubic_line(N);
    assert!(
        edges > 262_144 && edges <= 524_288,
        "the fixture is not between the old and the new bound: {edges} edges"
    );
    let actual = product(&path, W, H, false).unwrap_or_else(|e| panic!("the sweep refused: {e}"));
    let tile = reference(&Path::new().cubic_ring(7.3, 7.1, 6.0), 14, H, false).unwrap();
    let mut expected = vec![0u16; (W * H) as usize];
    for i in 0..N as usize {
        for y in 0..H as usize {
            for c in 0..14 {
                let at = y * W as usize + 13 * i + c;
                expected[at] = expected[at].saturating_add(tile[y * 14 + c]);
            }
        }
    }
    assert_within_one(&actual, &expected, W, "the 1,000-cubic-ring line");
    assert!(actual.contains(&65535), "no ink");

    let (over, edges) = cubic_line(1100);
    assert!(
        edges > 524_288,
        "the fixture is not past the bound: {edges}"
    );
    assert!(over.points.len() <= 262_144 && over.commands.len() <= 262_144);
    assert_eq!(
        product(&over, 13 * 1100 + 14, H, false).err().as_deref(),
        Some("artwork outline exceeds the flattened edge limit")
    );
}

/// 7,999 closed vertical subpaths on 16,000×1 (E = 15,998, no column or pair
/// breaks, breaks = {0, 1}): the full scan's total is `E·H + n` 31,998 +
/// `active²` 255,936,004 + `breaks·(active + 1)` 31,998 = 256,000,000
/// EXACTLY, which `charge` accepts. The earlier sweep's was 255,984,001 (one
/// row-wide pair sweep, `Σ (2·|open| + 1)` = `active²`); now each
/// zero-width subpath is its own x-cluster, so the sweep's is 87,991: `n`
/// 16,000 + `Σ active` 15,998 + per cluster a pair term of 4, one interval of
/// `|spanning|` 2 and 1 for the cluster. It must accept too. The
/// ≈128M-iteration pair loop runs once, in the reference (≈1–2 s,
/// load-independent).
#[test]
fn an_outline_at_exactly_the_cap_is_accepted_by_both_paths() {
    let mut path = Path::new();
    for k in 0..7999 {
        let x = f64::from(k) + 0.5;
        path.commands.extend([0, 1, 4]);
        path.points.extend([[x, -1.0], [x, 2.0]]);
    }
    let expected = reference(&path, 16_000, 1, false)
        .unwrap_or_else(|e| panic!("the full scan refused the boundary: {e}"));
    let actual = product(&path, 16_000, 1, false)
        .unwrap_or_else(|e| panic!("the sweep refused what the full scan accepts: {e}"));
    assert!(actual == expected, "the boundary outputs differ");
    assert!(actual.iter().all(|&v| v == 0));
}

/// `phase8e_coverage.rs`'s 18,000-edge 1×1 fixture: 9,000 zero-area loops of
/// two parallel edges, every edge spanning the one row in one x-cluster. The
/// reference's up-front `active²` term (324M) refuses. The sweep used to
/// refuse too, its running `Σ (2·|open| + 1)` reaching the same 324M with no
/// crossing to find; now full × full pairs are charged only when the
/// inversion sweep reports them, and no pair here is inverted, so it
/// finishes with the loops' zero area.
#[test]
fn the_parallel_outline_finishes_on_the_sweep_and_still_refuses_on_the_full_scan() {
    let mut path = Path::new();
    for index in 0..9000 {
        let x = f64::from(index) / 9000.0;
        path.points.extend([[x, 0.0], [x + 0.5, 1.0]]);
        path.commands.extend([0, 1, 4]);
    }
    assert_eq!(
        reference(&path, 1, 1, false).err().as_deref(),
        Some(SENTENCE)
    );
    assert_eq!(product(&path, 1, 1, false), Ok(vec![0]));
}

/// A hatch past the reference's cap with an area to integrate: 9,000
/// parallelogram strips of width `w = 1/36,000` at `x = k/18,000`, slanted
/// half a pixel across the 1×1 row, 18,000 full edges in one cluster, none
/// crossing. Each strip lies inside the pixel (its right end is below 1), so
/// the covered area is `9,000·w` exactly in real arithmetic and the sample is
/// that times 65535, rounded; the sweep's float summation may differ by one
/// code. The reference refuses at `active²`.
#[test]
fn a_hatch_of_parallel_strips_past_the_cap_integrates_its_area() {
    const STRIPS: u32 = 9000;
    let w = 1.0 / 36_000.0;
    let mut path = Path::new();
    for k in 0..STRIPS {
        let x = f64::from(k) / 18_000.0;
        path = path.polygon(&[[x, 0.0], [x + w, 0.0], [x + w + 0.5, 1.0], [x + 0.5, 1.0]]);
    }
    assert_eq!(
        reference(&path, 1, 1, false).err().as_deref(),
        Some(SENTENCE)
    );
    let expected = (f64::from(STRIPS) * w * 65535.0).round() as u16;
    let actual = product(&path, 1, 1, false).unwrap_or_else(|e| panic!("the sweep refused: {e}"));
    assert_within_one(&actual, &[expected], 1, "the hatch");
}

/// A small deterministic generator for the crossing fixtures.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn within(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.next()
    }
}

/// `count` triangles whose two slanted edges span every row of `[0, h)`
/// (vertices at y = −0.5 and h + 0.5, x seeded across `[−4, w + 4]`), so
/// their full edges cross one another many times inside rows, plus `small`
/// triangles with every vertex inside one row (partial edges).
fn crossing_field(seed: u64, count: u32, small: u32, w: u32, h: u32) -> Path {
    let mut rng = Lcg(seed);
    let (left, right) = (-4.0, f64::from(w) + 4.0);
    let below = f64::from(h) + 0.5;
    let mut path = Path::new();
    for _ in 0..count {
        let a = rng.within(left, right);
        let b = rng.within(left, right);
        let c = rng.within(left, right);
        path = path.polygon(&[[a, -0.5], [b, below], [c, below]]);
    }
    for _ in 0..small {
        let row = (rng.next() * f64::from(h)).floor();
        let x = rng.within(0.0, f64::from(w));
        let y = |v: f64| row + 0.1 + 0.8 * v;
        let (p, q, r) = (rng.next(), rng.next(), rng.next());
        path = path.polygon(&[
            [x, y(p)],
            [x + rng.within(-3.0, 3.0), y(q)],
            [x + 1.5, y(r)],
        ]);
    }
    path
}

/// Crossing full edges inside the cap, equal to the full scan sample for
/// sample: 300 triangles (600 full edges, about a quarter of their pairs
/// crossing inside the row) on a wide one-row canvas, then 100 on six rows,
/// each with partial triangles mixed in. A pair stage that reported no full ×
/// full pair would miss those breakpoints, and the interval sweep would
/// integrate across the crossings.
#[test]
fn crossing_full_edges_equal_the_full_scan() {
    let one_row = crossing_field(0x5eed_0001, 300, 40, 64, 1);
    assert_exact_both(&one_row, 64, 1, "300 crossing triangles on one row");
    let rows = crossing_field(0x5eed_0002, 100, 30, 48, 6);
    assert_exact_both(&rows, 48, 6, "100 crossing triangles on six rows");
}

/// The cap is still a cap: 9,000 zero-area loops whose lines all pass through
/// (0.5, 0.5), so every pair of loops crosses inside the 1×1 row. The full
/// scan refuses at `active²`; the sweep reports every pair, charged 2 each,
/// and refuses too, at the work limit or, first, at the row's breakpoint
/// limit, its output untouched.
#[test]
fn a_fan_whose_full_edges_all_cross_still_refuses() {
    let mut path = Path::new();
    for index in 0..9000 {
        let x = f64::from(index) / 9000.0;
        path.points.extend([[x, 0.0], [1.0 - x, 1.0]]);
        path.commands.extend([0, 1, 4]);
    }
    assert_eq!(
        reference(&path, 1, 1, false).err().as_deref(),
        Some(SENTENCE)
    );
    let refusal = product(&path, 1, 1, false).expect_err("the fan finished");
    assert!(
        refusal == SENTENCE || refusal == "artwork row exceeds its integration breakpoint limit",
        "{refusal}"
    );
}

#[test]
fn the_reference_refuses_rows_outside_the_canvas() {
    let path = rect_path(1.0, 1.0, 2.0, 2.0);
    for rows in [(3, 2), (0, 5)] {
        assert!(reference_rasterize(&path.commands, &path.points, 4, 4, false, rows).is_err());
    }
    let band = reference_rasterize(&path.commands, &path.points, 4, 4, false, (1, 3)).unwrap();
    assert_eq!(band.len(), 8);
}

// ------------------------------------------------------------- reports --

fn edit(path: Path, w: u32, h: u32) -> impl FnOnce(usize) -> (bool, String) {
    move |token| match product_with(&path, w, h, false, token as *const DrJobControl) {
        Ok(_) => (true, String::new()),
        Err(e) => (false, e),
    }
}

/// 100 visited rows plus the leading and trailing skip blocks (both
/// non-empty): exactly 102 stage-5 reports.
#[test]
fn the_sweep_reports_once_per_visited_row_and_once_per_skipped_block() {
    const W: u32 = 64;
    const H: u32 = 8000;
    let path = || rect_path(10.25, 3000.5, 50.75, 3099.5);
    let log = reports(edit(path(), W, H));
    let stage5 = log.iter().filter(|r| r.stage == 5).collect::<Vec<_>>();
    assert_eq!(stage5.len(), 102, "stage-5 reports");
    assert!((stage5[0].progress - 3000. / 8000.).abs() < 1e-6);
    assert!((stage5[101].progress - 1.0).abs() < 1e-6);
    assert_exact(&path(), W, H, false, "reports fixture");
}

// ------------------------------------------------------------ cancellation --

const PW: u32 = 64;
const PH: u32 = 2000;

fn parked_path() -> Path {
    rect_path(0.13, 0.27, 63.71, 1999.83)
}

#[test]
fn a_pre_cancelled_token_returns_nothing_and_holds_nothing() {
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let result = product_with(&parked_path(), PW, PH, false, control);
    let scratch = unsafe { dr_job_control_scratch_bytes(control) };
    unsafe { dr_job_control_free(control) };
    let e = result.expect_err("a pre-cancelled coverage completed");
    assert!(e.contains("cancel"), "{e}");
    assert_eq!(scratch, 0);
}

#[test]
fn a_cancel_parked_inside_the_row_loop_returns_nothing_then_completes_exactly() {
    let n0 = entry_count(
        &|progress, stage, _| stage == 5 && progress > 0.,
        edit(parked_path(), PW, PH),
    );
    let p = cancelled_parked_at(
        n0 + 1 + 40,
        &|progress, stage| stage == 5 && progress > 0. && progress < 0.5,
        edit(parked_path(), PW, PH),
    );
    eprintln!(
        "13A-6b parked cancel inside the sweep's row loop: {}",
        parked_line(&p)
    );
    // Cancelled, then complete: equal to the full scan.
    let control = unsafe { dr_job_control_create() };
    let again = product_with(&parked_path(), PW, PH, false, control);
    unsafe { dr_job_control_free(control) };
    let expected = reference(&parked_path(), PW, PH, false).unwrap();
    assert!(again.unwrap() == expected, "the completed re-run differs");
}

/// A one-row cluster whose pair stage is nearly all of its work: a rectangle
/// spanning the 1×1 row (two full vertical edges) around 1,500 zero-area
/// loops of two vertical partial edges from y = 0.25 to 0.75, so the 3,000
/// partial edges all overlap in y, none crosses another, and the row has
/// three intervals and no column breaks. The pair stage runs 4,504,500 pair
/// tests.
fn pair_loop_path() -> Path {
    let mut path = rect_path(0.0, -1.0, 1.0, 2.0);
    for k in 0..1500 {
        let x = f64::from(k + 1) / 1502.0;
        path.points.extend([[x, 0.25], [x, 0.75]]);
        path.commands.extend([0, 1, 4]);
    }
    path
}

/// The pair stage polls every 4,096 pair tests, not once per 4,096 edges. The
/// outline polls once per path command, and everything else in this 1×1 row
/// a handful of times, so the run's checkpoints beyond its command count are
/// the pair stage's: about 1,100 (4,504,500 / 4,096), where the earlier
/// stage polled once for the row's 3,002 edges. A cancel PARKED 500
/// checkpoints past the outline lands inside the pair loop (no report has
/// come yet: stage 1, progress 0), returns "operation cancelled" with the
/// output untouched, and a fresh run equals the full scan.
#[test]
fn the_pair_stage_polls_inside_its_loop_and_a_cancel_parked_there_returns_nothing() {
    let commands = pair_loop_path().commands.len() as u64;
    let total = entry_count(
        &|progress, stage, _| stage == 5 && progress >= 1.0,
        edit(pair_loop_path(), 1, 1),
    );
    assert!(
        total >= commands + 1000,
        "the run polled {} times beyond its {commands} path commands",
        total.saturating_sub(commands)
    );
    let p = cancelled_parked_at(
        commands + 500,
        &|progress, stage| stage == 1 && progress == 0.0,
        edit(pair_loop_path(), 1, 1),
    );
    eprintln!("parked cancel inside the pair stage: {}", parked_line(&p));
    let control = unsafe { dr_job_control_create() };
    let again = product_with(&pair_loop_path(), 1, 1, false, control);
    unsafe { dr_job_control_free(control) };
    let expected = reference(&pair_loop_path(), 1, 1, false).unwrap();
    assert_eq!(expected, vec![65535]);
    assert!(again.unwrap() == expected, "the completed re-run differs");
}

// ------------------------------------------------ bench (cancel-bench) --

/// The bench's render: the bare FFI call into a caller-owned output, so the
/// probe's return stamp does not include `product_with`'s O(n) sentinel scan
/// or the output's deallocation (both the harness's, not the sweep's).
fn bare_edit<'a>(
    path: &'a Path,
    w: u32,
    h: u32,
    out: &'a mut [u16],
) -> impl FnOnce(usize) -> (bool, String) + 'a {
    move |token| {
        let mut e = ptr::null_mut();
        let ok = unsafe {
            dr_artwork_path_coverage(
                path.commands.as_ptr(),
                path.commands.len(),
                path.points.as_ptr().cast(),
                path.points.len(),
                w,
                h,
                false,
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                token as *const DrJobControl,
                &mut e,
            )
        };
        (ok, unsafe { message(e) })
    }
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-6b)"]
fn a_concurrently_cancelled_sweep_returns_within_one_row() {
    // 2 active edges per row, one interval. `c_row` is one row from the
    // uncancelled progress log, between the first logged progress ≥ 0.05 and
    // the last ≤ 0.5.
    const W: u32 = 4096;
    const H: u32 = 8000;
    let path = || rect_path(0.13, 0.27, 4095.71, 7999.83);
    let (log, _) = logged(edit(path(), W, H));
    let (t1, p1) = *log.iter().find(|&&(_, v)| v >= 0.05).unwrap();
    let (t2, p2) = *log.iter().rev().find(|&&(_, v)| v <= 0.5).unwrap();
    let c_row = (t2 - t1).as_secs_f64() / ((p2 - p1) * f64::from(H));
    let fixture = path();
    let mut out = vec![SENTINEL; (W * H) as usize];
    let p = probe(
        &|progress, stage, _| stage == 5 && progress > 0.05,
        Trigger::After(500),
        &|progress, stage| stage == 5 && progress <= 0.6,
        bare_edit(&fixture, W, H, &mut out),
    );
    assert!(
        out.iter().all(|&v| v == SENTINEL),
        "a cancelled coverage wrote its output"
    );
    let bound = RETURN_FLOOR_S + c_row;
    eprintln!(
        "13A-6b sweep ({W}×{H}, 2 active edges per row) cancel inside the row loop: {:.3} ms \
         cancel-to-return (issued at {:.1} ms; {}); c_row {:.4} µs ({:.0} rows timed); bound \
         T_f + c_row = {:.3} ms (T_f 1 ms); ≈{:.2} rows past the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        c_row * 1e6,
        (p2 - p1) * f64::from(H),
        bound * 1e3,
        past(p.latency, c_row),
        verdict(p.latency, bound),
    );
}
