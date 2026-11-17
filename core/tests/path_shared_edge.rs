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
//!
//! HELD OPERATIONS. The routine that folds each subpath's own region by its
//! operation (`artwork_ops_coverage`, through its `ops_coverage` seam) is held
//! to the same cases: (a)'s two rectangles united are the one rectangle, the
//! left less the right is the left alone, the two intersected cover nothing
//! and excluded they are the union; (c) and (d) united are their outlines.
//! Overlapping rectangles under each operation, a first subtract, an order
//! case and two windings are held to their exact areas, computed here by
//! cutting each pixel at every rectangle side. The verbs and points are
//! written here from the node lists.

use doctored_core::artwork_ops_coverage::ops_coverage;
use doctored_core::path_geometry::SubpathOp::{self, Exclude, Intersect, Subtract, Unite};

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

/// How far a held-operations plane may sit from an exact area, in u16 codes.
const EXACT: u16 = 1;
/// How far (c) united may sit from the one rectangle, and (d) from the outer
/// outline: (c)'s and (d)'s bounds above.
const TRIANGLES: u16 = 1;
const CUBIC: u16 = 64;

/// The overlapping rectangles of the exact-area cases, `[x0, y0, x1, y1]`.
const A: [f64; 4] = [2.25, 3.0, 10.5, 9.75];
const B: [f64; 4] = [6.5, 5.5, 14.0, 12.0];
const C: [f64; 4] = [8.25, 7.5, 12.75, 10.25];

/// A region over rectangles: whether a point is in it, given whether it is
/// in each rectangle.
type Region = dyn Fn(&[bool]) -> bool;

/// One closed subpath's verbs and points from its nodes, as a fill outline
/// is written: a move to the first node; each segment, the closing one
/// included, a line when both of its handles sit on their points and a cubic
/// otherwise; then a close.
fn subpath_commands(nodes: &[Node], verbs: &mut Vec<u8>, points: &mut Vec<[f64; 2]>) {
    verbs.push(0);
    points.push(nodes[0].p);
    for k in 0..nodes.len() {
        let (a, b) = (nodes[k], nodes[(k + 1) % nodes.len()]);
        if a.o == a.p && b.i == b.p {
            verbs.push(1);
            points.push(b.p);
        } else {
            verbs.push(3);
            points.extend([a.o, b.i, b.p]);
        }
    }
    verbs.push(4);
}

/// The held-operations plane of closed `subpaths` folded by `ops` over the
/// 24 × 32 window, each subpath's own region under `even_odd`.
fn folded(subpaths: &[&[Node]], ops: &[SubpathOp], even_odd: bool) -> Result<Vec<u16>, String> {
    let (mut verbs, mut points) = (Vec::new(), Vec::new());
    for nodes in subpaths {
        subpath_commands(nodes, &mut verbs, &mut points);
    }
    ops_coverage(&verbs, &points, ops, SIZE.0, SIZE.1, even_odd)
}

fn ring(r: [f64; 4]) -> Vec<Node> {
    rectangle(r[0], r[1], r[2], r[3], false)
}

/// The exact covered area of pixel `(px, py)` of the region `inside` names
/// over `rects`' memberships: the pixel cut at every rectangle side crossing
/// it, so each cell lies wholly inside or outside each rectangle, and a cell
/// counted when `inside` holds at its centre.
fn exact_area(px: u32, py: u32, rects: &[[f64; 4]], inside: &Region) -> f64 {
    let cuts = |low: f64, sides: &mut dyn Iterator<Item = f64>| {
        let mut cut = vec![low, low + 1.0];
        cut.extend(sides.filter(|v| *v > low && *v < low + 1.0));
        cut.sort_by(f64::total_cmp);
        cut
    };
    let xs = cuts(f64::from(px), &mut rects.iter().flat_map(|r| [r[0], r[2]]));
    let ys = cuts(f64::from(py), &mut rects.iter().flat_map(|r| [r[1], r[3]]));
    let mut area = 0.0;
    for x in xs.windows(2) {
        for y in ys.windows(2) {
            let (cx, cy) = ((x[0] + x[1]) * 0.5, (y[0] + y[1]) * 0.5);
            let member: Vec<bool> = rects
                .iter()
                .map(|r| cx > r[0] && cx < r[2] && cy > r[1] && cy < r[3])
                .collect();
            if inside(&member) {
                area += (x[1] - x[0]) * (y[1] - y[0]);
            }
        }
    }
    area
}

/// [`exact_area`] over the whole window, as u16 samples.
fn exact_plane(rects: &[[f64; 4]], inside: &Region) -> Vec<u16> {
    (0..SIZE.1)
        .flat_map(|y| (0..SIZE.0).map(move |x| (x, y)))
        .map(|(x, y)| (exact_area(x, y, rects, inside) * 65535.0).round() as u16)
        .collect()
}

#[test]
fn held_operations_on_two_rectangles_sharing_an_edge_leave_no_seam() {
    let whole = plane(&[&rectangle(4.0, 3.0, 17.0, 28.0, false)], "nonzero");
    let left = rectangle(4.0, 3.0, SEAM_X, 28.0, false);
    let left_alone = plane(&[&left], "nonzero");
    let nothing = vec![0u16; whole.len()];
    for (label, even_odd, reversed) in [
        ("nonzero, same orientation", false, false),
        ("nonzero, opposite orientations", false, true),
        ("evenodd", true, false),
        ("evenodd, opposite orientations", true, true),
    ] {
        let right = rectangle(SEAM_X, 3.0, 17.0, 28.0, reversed);
        for (op, oracle, seam_full) in [
            (Unite, &whole, true),
            (Subtract, &left_alone, false),
            (Intersect, &nothing, false),
            (Exclude, &whole, true),
        ] {
            let held = folded(&[&left, &right], &[Unite, op], even_odd).expect("held");
            let (most, at) = worst(&held, oracle);
            let column = seam(&held);
            println!(
                "(a) held {op:?}, {label}: seam column min {:?}, worst difference {most} at sample {at}",
                column.iter().min()
            );
            if seam_full {
                assert!(
                    column.iter().all(|v| *v == u16::MAX),
                    "(a) held {op:?}, {label}: the shared edge's column is not wholly covered: {column:?}"
                );
            }
            assert_eq!(
                most, 0,
                "(a) held {op:?}, {label}: sample {at} differs from its oracle by {most}"
            );
        }
    }
}

/// (d)'s two halves of `[3, 21] × [3, 29]`, split by the S-shaped cubic.
fn s_halves() -> (Vec<Node>, Vec<Node>) {
    let (top, bottom) = ([12.0, 3.0], [12.0, 29.0]);
    let (c1, c2) = ([20.0, 10.0], [4.0, 22.0]);
    let left = vec![
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
    let right = vec![
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
    (left, right)
}

#[test]
fn held_unite_of_triangles_and_of_cubic_halves_is_their_outline() {
    let (x0, y0, x1, y1) = (4.25, 3.5, 19.75, 27.5);
    let whole = plane(&[&rectangle(x0, y0, x1, y1, false)], "nonzero");
    let upper = [corner(x0, y0), corner(x1, y0), corner(x1, y1)];
    let lower = [corner(x0, y0), corner(x1, y1), corner(x0, y1)];
    let (left, right) = s_halves();
    let outer = plane(&[&rectangle(3.0, 3.0, 21.0, 29.0, false)], "nonzero");
    for even_odd in [false, true] {
        let pair = folded(&[&upper, &lower], &[Unite, Unite], even_odd).expect("(c)");
        let (most, at) = worst(&pair, &whole);
        println!("(c) held unite, even_odd {even_odd}: worst difference {most} at sample {at}");
        assert!(
            most <= TRIANGLES,
            "(c) held unite, even_odd {even_odd}: sample {at} differs by {most}"
        );
        let pair = folded(&[&left, &right], &[Unite, Unite], even_odd).expect("(d)");
        let (most, at) = worst(&pair, &outer);
        println!("(d) held unite, even_odd {even_odd}: worst difference {most} at sample {at}");
        assert!(
            most <= CUBIC,
            "(d) held unite, even_odd {even_odd}: sample {at} differs by {most}"
        );
    }
}

#[test]
fn held_operations_on_overlapping_rectangles_cover_their_exact_areas() {
    let (a, b) = (ring(A), ring(B));
    let cases: [(SubpathOp, &Region); 4] = [
        (Unite, &|m: &[bool]| m[0] || m[1]),
        (Subtract, &|m: &[bool]| m[0] && !m[1]),
        (Intersect, &|m: &[bool]| m[0] && m[1]),
        (Exclude, &|m: &[bool]| m[0] != m[1]),
    ];
    for (op, inside) in cases {
        let exact = exact_plane(&[A, B], inside);
        for even_odd in [false, true] {
            let held = folded(&[&a, &b], &[Unite, op], even_odd).expect("held");
            let (most, at) = worst(&held, &exact);
            println!("held {op:?}, even_odd {even_odd}: worst difference from the exact area {most} at sample {at}");
            assert!(
                most <= EXACT,
                "held {op:?}, even_odd {even_odd}: sample {at} differs from the exact area by {most}"
            );
            if op == Intersect {
                // B misses rows 0…4 and 12…31: the intersect clears them.
                let missed = (0..5).chain(12..SIZE.1);
                for y in missed {
                    let row = &held[(y * SIZE.0) as usize..((y + 1) * SIZE.0) as usize];
                    assert!(
                        row.iter().all(|v| *v == 0),
                        "held intersect, row {y}: {row:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_first_subtract_starts_from_everything() {
    let exact = exact_plane(&[A], &|m: &[bool]| !m[0]);
    for even_odd in [false, true] {
        let held = folded(&[&ring(A)], &[Subtract], even_odd).expect("held");
        let (most, at) = worst(&held, &exact);
        println!(
            "held subtract alone, even_odd {even_odd}: worst difference {most} at sample {at}"
        );
        assert!(
            most <= EXACT,
            "held subtract alone, even_odd {even_odd}: sample {at} differs by {most}"
        );
        // The rows the rectangle misses, above and below it, are wholly covered.
        for y in (0..3).chain(10..SIZE.1) {
            let row = &held[(y * SIZE.0) as usize..((y + 1) * SIZE.0) as usize];
            assert!(row.iter().all(|v| *v == u16::MAX), "row {y}: {row:?}");
        }
    }
}

#[test]
fn held_operations_fold_in_their_order() {
    let (a, b, c) = (ring(A), ring(B), ring(C));
    let first = exact_plane(&[A, B, C], &|m: &[bool]| (m[0] && !m[1]) || m[2]);
    let second = exact_plane(&[A, B, C], &|m: &[bool]| (m[0] || m[1]) && !m[2]);
    assert_ne!(first, second, "the two orders must name different regions");
    for even_odd in [false, true] {
        let held = folded(&[&a, &b, &c], &[Unite, Subtract, Unite], even_odd).expect("first");
        let (most, at) = worst(&held, &first);
        println!(
            "unite, subtract, unite, even_odd {even_odd}: worst difference {most} at sample {at}"
        );
        assert!(
            most <= EXACT,
            "unite, subtract, unite: sample {at} differs by {most}"
        );
        let held = folded(&[&a, &b, &c], &[Unite, Unite, Subtract], even_odd).expect("second");
        let (most, at) = worst(&held, &second);
        println!(
            "unite, unite, subtract, even_odd {even_odd}: worst difference {most} at sample {at}"
        );
        assert!(
            most <= EXACT,
            "unite, unite, subtract: sample {at} differs by {most}"
        );
    }
}

#[test]
fn each_subpath_takes_the_fill_rule_on_its_own_winding() {
    // One subpath tracing A twice: winding 2 inside it.
    let [x0, y0, x1, y1] = A;
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let verbs = [0, 1, 1, 1, 1, 1, 1, 1, 4];
    let points: Vec<[f64; 2]> = corners.iter().chain(&corners).copied().collect();
    let twice = |even_odd| ops_coverage(&verbs, &points, &[Unite], SIZE.0, SIZE.1, even_odd);
    let exact = exact_plane(&[A], &|m: &[bool]| m[0]);
    let (most, at) = worst(&twice(false).expect("nonzero"), &exact);
    println!("A traced twice, nonzero: worst difference {most} at sample {at}");
    assert!(
        most <= EXACT,
        "A traced twice, nonzero: sample {at} differs by {most}"
    );
    assert!(
        twice(true).expect("evenodd").iter().all(|v| *v == 0),
        "A traced twice covers nothing under even-odd"
    );

    // An outer clockwise and an inner counter-clockwise rectangle: their
    // union is the outer one, where the summed winding cuts a hole.
    let (outer, inner) = ([3.0, 3.0, 20.0, 28.0], [7.0, 8.0, 15.0, 20.0]);
    let inner_ring = rectangle(inner[0], inner[1], inner[2], inner[3], true);
    let exact = exact_plane(&[outer], &|m: &[bool]| m[0]);
    let held = folded(&[&ring(outer), &inner_ring], &[Unite, Unite], false).expect("held");
    let (most, at) = worst(&held, &exact);
    println!("outer and counter-wound inner, held unite: worst difference {most} at sample {at}");
    assert!(most <= EXACT, "held unite: sample {at} differs by {most}");
    let summed = plane(&[&ring(outer), &inner_ring], "nonzero");
    assert_eq!(
        summed[(12 * SIZE.0 + 10) as usize],
        0,
        "the summed winding cuts a hole"
    );
    assert_eq!(held[(12 * SIZE.0 + 10) as usize], u16::MAX);
}

#[test]
fn operations_that_do_not_match_the_subpaths_refuse() {
    let (a, b) = (ring(A), ring(B));
    for ops in [&[Unite][..], &[Unite, Unite, Unite][..]] {
        assert_eq!(
            folded(&[&a, &b], ops, false).unwrap_err(),
            "artwork path operations do not match its subpaths"
        );
    }
}
