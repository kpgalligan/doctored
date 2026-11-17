//! A SHARED EDGE between two subpaths of one path, through `dr_path_coverage`:
//! the measurement `designs/v1-paths-held-subpaths.md` rests on. Two closed
//! subpaths that meet along an edge cover the pixels on it exactly as the one
//! outline they make together does, whatever their orientations and the fill
//! rule, because the rasterizer applies the fill rule to each run between two
//! crossings (`artwork_coverage`'s interval walk) rather than to a pixel's
//! summed area. The seam a held subpath operation would leave comes only from
//! combining each subpath's OWN anti-aliased coverage per pixel, which (b)
//! reproduces with the formulas written out here.
//!
//! The oracle is always another outline's coverage from the same export, or
//! the exact area of a half-covered pixel; the bounds are fixed here before
//! anything is measured and are never widened. Each test prints its worst
//! sample (`make t F=path_shared_edge A=--nocapture`).

use crate::path_shape::{corner, coverage, shape_of, Node};

/// The 24 × 32 window every case draws, from canvas pixel (0, 0).
const SIZE: (u32, u32) = (24, 32);
/// The shared vertical edge of (a) and (b), through the middle of column 10.
const SEAM_X: f64 = 10.5;
const SEAM_COLUMN: u32 = 10;
/// Rows 3…27, the rows wholly inside (a)'s and (b)'s rectangles.
const ROWS: std::ops::Range<u32> = 3..28;

/// The corners of the rectangle `[x0, x1] × [y0, y1]`, clockwise on screen
/// (y down) or, with `reversed`, counter-clockwise.
fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64, reversed: bool) -> Vec<Node> {
    let mut ring = vec![
        corner(x0, y0),
        corner(x1, y0),
        corner(x1, y1),
        corner(x0, y1),
    ];
    if reversed {
        ring.reverse();
    }
    ring
}

/// The anti-aliased plane of `subpaths` (all closed) under `rule`.
fn plane(subpaths: &[&[Node]], rule: &str) -> Vec<u16> {
    let closed: Vec<(&[Node], bool)> = subpaths.iter().map(|s| (*s, true)).collect();
    coverage(&shape_of(&closed, rule), (0, 0), SIZE, true, None).expect("coverage")
}

/// The largest sample difference between two planes, and where.
fn worst(a: &[u16], b: &[u16]) -> (u16, usize) {
    a.iter()
        .zip(b)
        .enumerate()
        .map(|(i, (p, q))| (p.abs_diff(*q), i))
        .max()
        .unwrap_or((0, 0))
}

/// The samples of column 10, rows 3…27.
fn seam(samples: &[u16]) -> Vec<u16> {
    ROWS.map(|y| samples[(y * SIZE.0 + SEAM_COLUMN) as usize])
        .collect()
}

#[test]
fn two_rectangles_sharing_an_edge_cover_it_as_the_one_rectangle_does() {
    let whole = plane(&[&rectangle(4.0, 3.0, 17.0, 28.0, false)], "nonzero");
    let left = rectangle(4.0, 3.0, SEAM_X, 28.0, false);
    for (label, rule, reversed) in [
        ("nonzero, same orientation", "nonzero", false),
        ("nonzero, opposite orientations", "nonzero", true),
        ("evenodd", "evenodd", false),
        ("evenodd, opposite orientations", "evenodd", true),
    ] {
        let right = rectangle(SEAM_X, 3.0, 17.0, 28.0, reversed);
        let pair = plane(&[&left, &right], rule);
        let (most, at) = worst(&pair, &whole);
        let column = seam(&pair);
        println!("(a) {label}: seam column min {:?}, worst difference from the one rectangle {most} at sample {at}",
                 column.iter().min());
        assert!(
            column.iter().all(|v| *v == u16::MAX),
            "(a) {label}: the shared edge's column is not wholly covered: {column:?}"
        );
        assert_eq!(
            most, 0,
            "(a) {label}: sample {at} differs from the one rectangle by {most}"
        );
    }
}

#[test]
fn per_subpath_coverages_combined_per_pixel_leave_the_quarter_seam() {
    // Each rectangle alone, then `vector_mask_combine`'s formulas on the two
    // fractions: union c + s − c·s, subtract c·(1 − s).
    let left = plane(&[&rectangle(4.0, 3.0, SEAM_X, 28.0, false)], "nonzero");
    let right = plane(&[&rectangle(SEAM_X, 3.0, 17.0, 28.0, false)], "nonzero");
    let combine = |f: fn(f64, f64) -> f64| -> Vec<u16> {
        left.iter()
            .zip(&right)
            .map(|(c, s)| {
                let (c, s) = (f64::from(*c) / 65535.0, f64::from(*s) / 65535.0);
                (f(c, s).clamp(0.0, 1.0) * 65535.0).round() as u16
            })
            .collect()
    };
    let union = seam(&combine(|c, s| c + s - c * s));
    let subtract = seam(&combine(|c, s| c * (1.0 - s)));
    let exact = seam(&left);
    println!(
        "(b) union {:?}…{:?}, subtract {:?}…{:?}, the left rectangle alone {:?}…{:?}",
        union.iter().min(),
        union.iter().max(),
        subtract.iter().min(),
        subtract.iter().max(),
        exact.iter().min(),
        exact.iter().max()
    );
    // Half of each seam pixel is each rectangle's.
    assert!(
        exact.iter().all(|v| v.abs_diff(32768) <= 1),
        "(b) half coverage: {exact:?}"
    );
    // The union is three quarters where the region covers the whole pixel.
    assert!(
        union.iter().all(|v| v.abs_diff(49151) <= 1),
        "(b) union: {union:?}"
    );
    // The difference is one quarter where the region covers one half.
    assert!(
        subtract.iter().all(|v| v.abs_diff(16384) <= 1),
        "(b) subtract: {subtract:?}"
    );
}

#[test]
fn two_triangles_sharing_a_diagonal_cover_it_as_the_one_rectangle_does() {
    let (x0, y0, x1, y1) = (4.25, 3.5, 19.75, 27.5);
    let whole = plane(&[&rectangle(x0, y0, x1, y1, false)], "nonzero");
    // Corner to corner: both clockwise on screen, the diagonal walked
    // forward by one and backward by the other.
    let upper = [corner(x0, y0), corner(x1, y0), corner(x1, y1)];
    let lower = [corner(x0, y0), corner(x1, y1), corner(x0, y1)];
    for rule in ["nonzero", "evenodd"] {
        let pair = plane(&[&upper, &lower], rule);
        let (most, at) = worst(&pair, &whole);
        println!("(c) {rule}: worst difference from the one rectangle {most} at sample {at}");
        assert!(
            most <= 1,
            "(c) {rule}: sample {at} differs from the rectangle by {most}"
        );
    }
}

#[test]
fn two_subpaths_sharing_a_cubic_cover_it_as_their_outer_outline_does() {
    // An S-shaped cubic from the top middle to the bottom middle of
    // [3, 21] × [3, 29] splits it in two: the left subpath walks the cubic
    // forward, the right one walks it backward; both are clockwise on screen.
    let (top, bottom) = ([12.0, 3.0], [12.0, 29.0]);
    let (c1, c2) = ([20.0, 10.0], [4.0, 22.0]);
    let left = [
        Node {
            p: top,
            i: top,
            o: c1,
        },
        Node {
            p: bottom,
            i: c2,
            o: bottom,
        },
        corner(3.0, 29.0),
        corner(3.0, 3.0),
    ];
    let right = [
        Node {
            p: bottom,
            i: bottom,
            o: c2,
        },
        Node {
            p: top,
            i: c1,
            o: top,
        },
        corner(21.0, 3.0),
        corner(21.0, 29.0),
    ];
    let outer = plane(&[&rectangle(3.0, 3.0, 21.0, 29.0, false)], "nonzero");
    for rule in ["nonzero", "evenodd"] {
        let pair = plane(&[&left, &right], rule);
        let (most, at) = worst(&pair, &outer);
        println!("(d) {rule}: worst difference from the outer outline {most} at sample {at}");
        assert!(
            most <= 64,
            "(d) {rule}: sample {at} differs from the outer outline by {most}"
        );
    }
}
