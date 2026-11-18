//! The bake of a PSD vector mask Doctored does not carry
//! (`psd_vector_mask_bake`): one coverage of the shape with the file's
//! operations over the canvas, so two subpaths sharing an anti-aliased edge
//! leave no seam.
//!
//! Two oracles, their bound declared here before anything was measured: the
//! bake's u16 samples, read through the `vmsk_bake` seam, are held within
//! [`BOUND`] of each.
//!
//! - SUBCELLS, for axis-aligned rectangles, independent of the rasterizer.
//!   Each pixel is 16 × 16 subcells, and with every coordinate a multiple of
//!   1/16 px a subcell is wholly inside or outside each rectangle. The
//!   booleans fold per subcell in file order (0 exclude, 1 unite,
//!   2 subtract, 3 intersect), starting true when the initial fill record is
//!   1 or the file's FIRST subpath (one of a single anchor included)
//!   subtracts, skipping every subpath of one anchor, complemented last for
//!   flag bit 0. The expected sample is `round(count · 65535 / 256)`.
//! - THE HELD SHAPE, for curved and sloped subpaths: `dr_path_coverage` (the
//!   FFI's rasterizer of a shape, origin (0, 0)) over the whole canvas of
//!   ONE JSON shape, nonzero, each subpath of two or more anchors carrying
//!   its `op`, the others left out. Its start is said by a construction
//!   other than the bake's: where the mask's start (as above) differs from
//!   the one the shape's own rule gives (all pixels exactly when its first
//!   subpath subtracts), an edgeless one-anchor subpath comes first, one
//!   that subtracts to start from all pixels or unites to start from none.
//!   Inverted last, here.
//!
//! The canvas is 64 × 32 and every coordinate a multiple of 1/16 px, so an
//! 8.24 fraction of either side holds it exactly and the FFI's four-decimal
//! reading leaves it alone. The rectangle cases cover each way a mask
//! starts, each operation after a unite, the head of a mask from all pixels
//! (a unite, an intersect, an exclude first), one-anchor subpaths first and
//! later, the inverted flag, rectangles off the canvas and cut by its edges,
//! and masks whose every subpath is dropped; the held-shape cases a curved
//! subpath, sloped edges and subpaths cut by the canvas. One case goes end
//! to end through an 8-bit open, whose mask is the coverage rounded to
//! bytes.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_path::dr_path_coverage;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use doctored_core::psd_vector_mask_bake::vmsk_bake;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use crate::common::psd_photoshop::import_report;
use crate::common::take_err_string;
use crate::psd_mask_fixture as fixture;
use crate::psd_paths::byte_open;

const CANVAS: (u32, u32) = (64, 32);

/// How far a baked u16 sample may sit from either oracle (module doc).
const BOUND: u16 = 1;

/// One anchor: the handle arriving at it, its point, the handle leaving it,
/// each (x, y) in canvas pixels.
type Knot = [[f64; 2]; 3];

fn corner(x: f64, y: f64) -> Knot {
    [[x, y], [x, y], [x, y]]
}

fn corners(points: &[(f64, f64)]) -> Vec<Knot> {
    points.iter().map(|&(x, y)| corner(x, y)).collect()
}

/// The rectangle `[x0, x1] × [y0, y1]`, its corners clockwise on screen.
fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Knot> {
    corners(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

/// One 26-byte path record: the selector, then `body`, zero-padded.
fn record(selector: u16, body: &[u8]) -> Vec<u8> {
    let mut out = selector.to_be_bytes().to_vec();
    out.extend_from_slice(body);
    out.resize(26, 0);
    out
}

/// A closed subpath by Adobe's layout: its length record (the knot count,
/// the combine `operation`, then 1) and an unlinked knot per anchor, each of
/// its three points a (vertical, horizontal) pair of 8.24 fractions of the
/// canvas — exact for this file's coordinates (module doc).
fn subpath(knots: &[Knot], operation: i16) -> Vec<u8> {
    let mut length = (knots.len() as u16).to_be_bytes().to_vec();
    length.extend_from_slice(&operation.to_be_bytes());
    length.extend_from_slice(&1u16.to_be_bytes());
    let mut out = record(0, &length);
    let fraction = |v: f64, side: u32| {
        let scaled = v / f64::from(side) * 16_777_216.0;
        assert_eq!(scaled.fract(), 0.0, "{v} is not exact as 8.24");
        (scaled as i32).to_be_bytes()
    };
    for knot in knots {
        let mut body = Vec::new();
        for [x, y] in knot {
            body.extend_from_slice(&fraction(*y, CANVAS.1));
            body.extend_from_slice(&fraction(*x, CANVAS.0));
        }
        out.extend(record(2, &body));
    }
    out
}

/// One bake case: the `vmsk` flags (bit 0 inverted, bit 2 disabled), the
/// initial fill record, and each subpath with its operation word (0 exclude,
/// 1 unite, 2 subtract, 3 intersect).
struct Case {
    name: String,
    flags: u32,
    initial: u16,
    subpaths: Vec<(Vec<Knot>, i16)>,
}

impl Case {
    fn new(name: &str, flags: u32, initial: u16, subpaths: &[(&Vec<Knot>, i16)]) -> Self {
        Self {
            name: name.to_string(),
            flags,
            initial,
            subpaths: subpaths.iter().map(|(k, op)| ((*k).clone(), *op)).collect(),
        }
    }

    /// The `vmsk` body: version 3, the flags, the fill rule record, the
    /// initial fill record, then the subpaths.
    fn body(&self) -> Vec<u8> {
        let mut out = 3u32.to_be_bytes().to_vec();
        out.extend_from_slice(&self.flags.to_be_bytes());
        out.extend(record(6, &[]));
        out.extend(record(8, &self.initial.to_be_bytes()));
        for (knots, operation) in &self.subpaths {
            out.extend(subpath(knots, *operation));
        }
        out
    }

    /// Whether the mask starts from all pixels (module doc).
    fn full(&self) -> bool {
        self.initial == 1 || self.subpaths.first().map(|s| s.1) == Some(2)
    }

    /// The subpaths of two or more anchors, in file order.
    fn kept(&self) -> Vec<&(Vec<Knot>, i16)> {
        self.subpaths.iter().filter(|(k, _)| k.len() >= 2).collect()
    }

    /// `plane` complemented when flag bit 0 says inverted.
    fn inverted(&self, mut plane: Vec<u16>) -> Vec<u16> {
        if self.flags & 1 != 0 {
            for c in &mut plane {
                *c = u16::MAX - *c;
            }
        }
        plane
    }
}

/// The `[x0, y0, x1, y1]` four corners bound, asserted an axis-aligned
/// rectangle: the subcell oracle reads nothing else.
fn bounds(knots: &[Knot]) -> [f64; 4] {
    assert_eq!(knots.len(), 4, "a rectangle has four corners");
    let points: Vec<[f64; 2]> = knots
        .iter()
        .map(|k| {
            assert!(k[0] == k[1] && k[2] == k[1], "a corner");
            k[1]
        })
        .collect();
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % 4];
        assert!(a[0] == b[0] || a[1] == b[1], "an axis-aligned side");
    }
    let xs = points.iter().map(|p| p[0]);
    let ys = points.iter().map(|p| p[1]);
    [
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    ]
}

/// The subcell fold of a case of rectangles (module doc).
fn subcell_oracle(case: &Case) -> Vec<u16> {
    let rects: Vec<([f64; 4], i16)> = case
        .kept()
        .into_iter()
        .map(|(k, word)| (bounds(k), *word))
        .collect();
    let (start, invert) = (case.full(), case.flags & 1 != 0);
    let mut out = Vec::with_capacity((CANVAS.0 * CANVAS.1) as usize);
    for py in 0..CANVAS.1 {
        for px in 0..CANVAS.0 {
            let mut count = 0u32;
            for j in 0..16u32 {
                for i in 0..16u32 {
                    let x = f64::from(px) + (f64::from(i) + 0.5) / 16.0;
                    let y = f64::from(py) + (f64::from(j) + 0.5) / 16.0;
                    let folded = rects.iter().fold(start, |r, ([x0, y0, x1, y1], word)| {
                        let s = *x0 < x && x < *x1 && *y0 < y && y < *y1;
                        match word {
                            0 => r != s,
                            1 => r || s,
                            2 => r && !s,
                            3 => r && s,
                            _ => unreachable!(),
                        }
                    });
                    count += u32::from(folded != invert);
                }
            }
            out.push((f64::from(count) * 65535.0 / 256.0).round() as u16);
        }
    }
    out
}

/// A shape's coverage of the WHOLE canvas through `dr_path_coverage`.
fn whole_canvas(shape: &Value) -> Vec<u16> {
    let text = CString::new(shape.to_string()).unwrap();
    let mut out = vec![0u16; (CANVAS.0 * CANVAS.1) as usize];
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_path_coverage(
            text.as_ptr(),
            0,
            0,
            CANVAS.0,
            CANVAS.1,
            true,
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "coverage: {}", take_err_string(err));
    out
}

/// One closed JSON subpath combined by `op`.
fn json_subpath(knots: &[Knot], op: &str) -> Value {
    let anchors: Vec<Value> = knots
        .iter()
        .map(|[i, p, o]| json!({"x": p[0], "y": p[1], "in": i, "out": o}))
        .collect();
    json!({"closed": true, "anchors": anchors, "op": op})
}

/// The JSON name of a length record's operation word.
fn op_name(word: i16) -> &'static str {
    match word {
        0 => "exclude",
        1 => "unite",
        2 => "subtract",
        3 => "intersect",
        _ => unreachable!(),
    }
}

/// The held shape's coverage of a case (module doc).
fn held_oracle(case: &Case) -> Vec<u16> {
    let kept = case.kept();
    let full = case.full();
    let mut subpaths = Vec::new();
    if full != (kept.first().map(|s| s.1) == Some(2)) {
        // Edgeless, so it folds as nothing: it only says where the fold starts.
        let op = if full { "subtract" } else { "unite" };
        subpaths.push(json_subpath(&[corner(0.0, 0.0)], op));
    }
    subpaths.extend(kept.iter().map(|(k, word)| json_subpath(k, op_name(*word))));
    case.inverted(whole_canvas(
        &json!({"fill_rule": "nonzero", "subpaths": subpaths}),
    ))
}

/// The bake of a case, which must bake.
fn baked(case: &Case) -> Vec<u16> {
    vmsk_bake(&case.body(), CANVAS)
        .unwrap_or_else(|| panic!("{}: the body bakes", case.name))
        .unwrap_or_else(|e| panic!("{}: {e}", case.name))
}

/// The first samples (index, bake, oracle) further than [`BOUND`] apart.
fn differences(got: &[u16], want: &[u16]) -> Vec<(usize, u16, u16)> {
    assert_eq!(got.len(), want.len(), "a canvas-sized plane");
    got.iter()
        .zip(want)
        .enumerate()
        .filter(|(_, (g, w))| g.abs_diff(**w) > BOUND)
        .map(|(i, (g, w))| (i, *g, *w))
        .take(5)
        .collect()
}

/// Every case held to `oracle`; the failures are listed together.
fn assert_cases(cases: &[Case], oracle: fn(&Case) -> Vec<u16>) {
    let failed: Vec<String> = cases
        .iter()
        .filter_map(|case| {
            let differ = differences(&baked(case), &oracle(case));
            (!differ.is_empty()).then(|| format!("{}: (index, bake, oracle) {differ:?}", case.name))
        })
        .collect();
    assert!(
        failed.is_empty(),
        "{} of {} cases:\n{}",
        failed.len(),
        cases.len(),
        failed.join("\n")
    );
}

/// Two rectangles meeting at x = 10.5, through the middle of column 10, in a
/// mask from all pixels (an initial fill of 1, so it is not carried and
/// reaches the bake): all ∩ left, then ∪ right. One coverage of the shape
/// covers the shared edge's column wholly; the per-pixel product of the two
/// halves would give it three quarters (49152).
#[test]
fn subpaths_sharing_an_edge_bake_without_a_seam() {
    let left = rectangle(4.0, 3.0, 10.5, 28.0);
    let right = rectangle(10.5, 3.0, 17.0, 28.0);
    let case = Case::new("a shared edge", 0, 1, &[(&left, 3), (&right, 1)]);
    let got = baked(&case);
    let seam: Vec<u16> = (3..28).map(|y| got[(y * CANVAS.0 + 10) as usize]).collect();
    assert!(
        seam.iter().all(|&v| v == u16::MAX),
        "the shared edge's column is not wholly covered: {seam:?}"
    );
    let differ = differences(&got, &subcell_oracle(&case));
    assert!(differ.is_empty(), "(index, bake, oracle) {differ:?}");
}

/// The rectangle fixtures, every coordinate a multiple of 1/16.
struct Rects {
    a: Vec<Knot>,
    /// Overlaps `a`, no edge shared.
    b: Vec<Knot>,
    /// Overlaps `a` and `b`; its left edge (x = 20.25) crosses pixel column
    /// 20 beside `a`'s right edge (x = 20.75), where the per-pixel product
    /// of the two would not be their fold.
    c: Vec<Knot>,
    /// Cut by the canvas's left edge.
    left: Vec<Knot>,
    /// Cut by the canvas's bottom-right corner.
    corner: Vec<Knot>,
    /// Wholly right of the canvas.
    off: Vec<Knot>,
    /// One anchor: encloses nothing and is skipped.
    dot: Vec<Knot>,
}

fn rects() -> Rects {
    Rects {
        a: rectangle(5.25, 4.5, 20.75, 18.125),
        b: rectangle(12.0625, 9.875, 30.5, 26.4375),
        c: rectangle(20.25, 2.75, 28.5625, 12.5),
        left: rectangle(-6.5, 20.25, 7.375, 30.0),
        corner: rectangle(55.125, 24.5, 70.0, 40.0),
        off: rectangle(70.0, 2.0, 80.0, 12.0),
        dot: vec![corner(16.0, 12.0)],
    }
}

fn rectangle_cases() -> Vec<Case> {
    let r = rects();
    let mut cases = Vec::new();
    for word in 0..4 {
        let op = op_name(word);
        cases.push(Case::new(
            &format!("{op} after a unite"),
            0,
            0,
            &[(&r.a, 1), (&r.b, word)],
        ));
        cases.push(Case::new(
            &format!("from all pixels, a first {op}"),
            0,
            1,
            &[(&r.a, word), (&r.b, 2)],
        ));
        cases.push(Case::new(
            &format!("from all pixels, a first {op}, then an exclude"),
            0,
            1,
            &[(&r.a, word), (&r.c, 0)],
        ));
        for (shape, place) in [
            (&r.off, "off the canvas"),
            (&r.left, "cut by the left edge"),
            (&r.corner, "cut by the corner"),
        ] {
            cases.push(Case::new(
                &format!("{op} {place}"),
                0,
                0,
                &[(&r.a, 1), (&r.c, 1), (shape, word)],
            ));
            cases.push(Case::new(
                &format!("{op} {place}, from all pixels"),
                0,
                1,
                &[(shape, word)],
            ));
        }
    }
    cases.extend([
        Case::new(
            "from all pixels, two unites, then an intersect",
            0,
            1,
            &[(&r.a, 1), (&r.c, 1), (&r.b, 3)],
        ),
        Case::new(
            "from all pixels, an intersect, then an exclude and a subtract",
            0,
            1,
            &[(&r.a, 3), (&r.b, 0), (&r.c, 2)],
        ),
        Case::new(
            "a first subtract beside a one-anchor subpath",
            0,
            0,
            &[(&r.a, 2), (&r.dot, 1), (&r.b, 1)],
        ),
        Case::new("a one-anchor intersect", 0, 0, &[(&r.a, 1), (&r.dot, 3)]),
        Case::new(
            "a one-anchor first subtract, then a unite",
            0,
            0,
            &[(&r.dot, 2), (&r.a, 1)],
        ),
        Case::new(
            "a one-anchor first subtract, then a unite and a subtract",
            0,
            0,
            &[(&r.dot, 2), (&r.a, 1), (&r.b, 2)],
        ),
        Case::new(
            "a one-anchor first unite, then a subtract and a unite",
            0,
            0,
            &[(&r.dot, 1), (&r.a, 2), (&r.b, 1)],
        ),
        Case::new("inverted", 1, 0, &[(&r.a, 1), (&r.b, 2)]),
        Case::new("inverted from all pixels", 1, 1, &[(&r.a, 3)]),
        Case::new("inverted, a first subtract", 1, 0, &[(&r.a, 2), (&r.b, 0)]),
        Case::new(
            "from all pixels, every subpath a unite",
            0,
            1,
            &[(&r.a, 1), (&r.b, 1)],
        ),
        Case::new("one anchor alone, a unite", 0, 0, &[(&r.dot, 1)]),
        Case::new("one anchor alone, a subtract", 0, 0, &[(&r.dot, 2)]),
        Case::new("one anchor alone, from all pixels", 0, 1, &[(&r.dot, 1)]),
    ]);
    cases
}

#[test]
fn masks_of_rectangles_bake_as_their_subcells_fold() {
    assert_cases(&rectangle_cases(), subcell_oracle);
}

/// The curved and sloped fixtures (module doc), every coordinate a multiple
/// of 1/16.
struct Shapes {
    /// A curved closed subpath, four smooth anchors.
    blob: Vec<Knot>,
    triangle: Vec<Knot>,
    /// Overlaps the blob and the triangle.
    big: Vec<Knot>,
    /// Inside the blob.
    small: Vec<Knot>,
    /// Right of the blob, disjoint from it.
    far: Vec<Knot>,
    /// Cut by the canvas's left edge.
    left: Vec<Knot>,
    /// Cut by the canvas's bottom-right corner.
    corner: Vec<Knot>,
    /// Wholly right of the canvas.
    off: Vec<Knot>,
    /// One anchor: encloses nothing and is skipped.
    dot: Vec<Knot>,
}

fn shapes() -> Shapes {
    Shapes {
        blob: vec![
            [[10.5, 15.0625], [10.5, 12.25], [10.5, 9.4375]],
            [[12.125, 5.75], [16.5, 5.75], [20.875, 5.75]],
            [[23.25, 9.4375], [23.25, 12.25], [23.25, 15.0625]],
            [[20.875, 18.5], [16.5, 18.5], [12.125, 18.5]],
        ],
        triangle: corners(&[(14.25, 3.5), (30.75, 9.125), (18.0625, 24.875)]),
        big: corners(&[(18.0, 9.0), (34.5, 10.25), (30.0, 28.75), (16.25, 22.5)]),
        small: corners(&[(14.0, 10.0), (19.5625, 10.5), (17.0, 15.25)]),
        far: corners(&[(40.125, 4.5), (52.875, 6.25), (49.5, 14.75), (41.0, 13.0)]),
        left: corners(&[(-8.5, 20.25), (6.75, 21.0), (5.125, 27.5), (-8.5, 30.0)]),
        corner: corners(&[(56.25, 26.5), (70.0, 28.0), (66.0, 40.0), (58.5, 35.0)]),
        off: corners(&[(70.0, 2.0), (80.0, 2.0), (75.0, 12.0)]),
        dot: vec![corner(16.0, 12.0)],
    }
}

fn cases() -> Vec<Case> {
    let s = shapes();
    let mut cases = vec![
        Case::new("add", 0, 0, &[(&s.blob, 1), (&s.triangle, 1)]),
        Case::new("subtract", 0, 0, &[(&s.blob, 1), (&s.big, 2)]),
        Case::new("intersect", 0, 0, &[(&s.blob, 1), (&s.big, 3)]),
        Case::new("exclude", 0, 0, &[(&s.blob, 1), (&s.big, 0)]),
        Case::new("exclude first", 0, 0, &[(&s.triangle, 0), (&s.blob, 0)]),
        Case::new(
            "disjoint subpaths",
            0,
            0,
            &[(&s.blob, 1), (&s.far, 1), (&s.left, 1), (&s.corner, 1)],
        ),
        Case::new(
            "an intersect inside a wider mask",
            0,
            0,
            &[(&s.blob, 1), (&s.far, 1), (&s.small, 3)],
        ),
        Case::new(
            "an add after an intersect",
            0,
            0,
            &[(&s.blob, 1), (&s.small, 3), (&s.far, 1), (&s.big, 0)],
        ),
        Case::new(
            "from all pixels",
            0,
            1,
            &[(&s.blob, 2), (&s.far, 3), (&s.triangle, 0)],
        ),
        Case::new(
            "from all pixels, cut",
            0,
            1,
            &[(&s.left, 0), (&s.corner, 2)],
        ),
        Case::new(
            "a first subtract",
            0,
            0,
            &[(&s.blob, 2), (&s.triangle, 1), (&s.small, 0)],
        ),
        Case::new(
            "a first subtract, then intersects",
            0,
            0,
            &[(&s.blob, 2), (&s.far, 3), (&s.small, 3)],
        ),
        Case::new("inverted", 1, 0, &[(&s.blob, 1), (&s.big, 2)]),
        Case::new("inverted from all pixels", 1, 1, &[(&s.blob, 3)]),
        Case::new("a one-anchor intersect", 0, 0, &[(&s.blob, 1), (&s.dot, 3)]),
        Case::new(
            "a one-anchor first subtract",
            0,
            0,
            &[(&s.dot, 2), (&s.triangle, 2)],
        ),
    ];
    for word in 0..4 {
        let op = op_name(word);
        for (shape, place) in [
            (&s.off, "off the canvas"),
            (&s.left, "cut by the left edge"),
            (&s.corner, "cut by the corner"),
        ] {
            cases.push(Case::new(
                &format!("{op} {place}"),
                0,
                0,
                &[(&s.blob, 1), (&s.far, 1), (shape, word)],
            ));
            cases.push(Case::new(
                &format!("{op} {place}, from all pixels"),
                0,
                1,
                &[(shape, word)],
            ));
        }
    }
    cases
}

#[test]
fn masks_of_curved_and_sloped_subpaths_bake_as_the_held_shape() {
    assert_cases(&cases(), held_oracle);
}

/// An 8-bit open draws the mask a raster record's uncarried `vmsk` holds
/// (one that starts from all pixels is not carried) into the layer's mask:
/// the held shape's coverage, each sample rounded to the nearest byte, each
/// byte within one of it.
#[test]
fn an_eight_bit_open_draws_the_bake_into_the_layers_mask() {
    let s = shapes();
    let case = Case::new(
        "open",
        0,
        1,
        &[(&s.blob, 1), (&s.far, 1), (&s.big, 2), (&s.small, 0)],
    );
    let mut record = fixture::Record::raster([0, 0, 4, 4], &[9, 9, 9, 255].repeat(16), false);
    record.blocks = fixture::block(b"vmsk", &case.body());
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("baked.psd");
    std::fs::write(
        &file,
        fixture::fixture(&[record], [CANVAS.0, CANVAS.1], false),
    )
    .unwrap();
    let losses = common::psd_photoshop::strings(&import_report(&file), "verified_losses");
    assert!(
        losses.iter().any(|l| l.contains("drawn from its path")),
        "{losses:?}"
    );
    let want: Vec<u8> = held_oracle(&case)
        .iter()
        .map(|&c| ((u32::from(c) * 255 + 32767) / 65535) as u8)
        .collect();
    let doc = byte_open(&file);
    let mask = unsafe { &*doc }.layers[0]
        .mask
        .as_ref()
        .map(|plane| plane.samples().clone());
    unsafe { dr_doc_free(doc) };
    let mask = mask.expect("the drawn mask");
    assert_eq!(mask.len(), want.len());
    let differ: Vec<(usize, u8, u8)> = mask
        .iter()
        .zip(&want)
        .enumerate()
        .filter(|(_, (g, w))| g.abs_diff(**w) > 1)
        .map(|(i, (g, w))| (i, *g, *w))
        .take(5)
        .collect();
    assert!(differ.is_empty(), "(index, mask, oracle) {differ:?}");
}

/// A body naming an operation word Doctored does not know, or one whose
/// flags say disabled (bit 2: it masks nothing), is not drawn.
#[test]
fn an_unknown_word_or_a_disabled_mask_is_not_drawn() {
    let r = rects();
    let unknown = Case::new("word 4", 0, 1, &[(&r.a, 1), (&r.b, 4)]);
    assert!(vmsk_bake(&unknown.body(), CANVAS).is_none(), "word 4");
    let disabled = Case::new("disabled", 4, 1, &[(&r.a, 2)]);
    assert!(vmsk_bake(&disabled.body(), CANVAS).is_none(), "disabled");
}
