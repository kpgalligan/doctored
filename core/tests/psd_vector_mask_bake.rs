//! The bake of a PSD vector mask Doctored does not carry
//! (`psd_vector_mask_bake`, `vector_mask_combine`): each subpath drawn over
//! its own window gives the picture the whole-canvas draw gives.
//!
//! Oracle: that whole-canvas draw, written here. Each subpath's coverage of
//! the WHOLE canvas comes from `dr_path_coverage` (the FFI's rasterizer of a
//! shape, origin (0, 0)), and the test combines them by the module doc's
//! four formulas — add `c + s - c s`, subtract `c (1 - s)`, intersect `c s`,
//! exclude `c + s - 2 c s`, rounded to a u16 after each subpath — from no
//! pixels, or from all of them for an initial fill record of 1 or a first
//! subpath that subtracts, inverted last for flag bit 0; a subpath of one
//! anchor is skipped. The bake's samples are read through the
//! `vmsk_bake` seam, u16 for u16, and one case goes end to end through an
//! 8-bit open, whose mask is that coverage rounded to bytes.
//!
//! The canvas is 64 × 32 and every coordinate a multiple of 1/16 px, so an
//! 8.24 fraction of either side holds it exactly and the FFI's four-decimal
//! reading leaves it alone: both draws rasterize the same numbers, and any
//! difference is the window's. Every case is asserted exact.
//!
//! The fixtures cover a curved subpath, sloped edges, subpaths cut by the
//! canvas's left edge and by its bottom-right corner, one wholly off the
//! canvas under each operation, disjoint windows, an intersect whose window
//! is smaller than what the mask held, each way a mask starts, the
//! inverted flag, and a one-anchor subpath.

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

/// One anchor: the handle arriving at it, its point, the handle leaving it,
/// each (x, y) in canvas pixels.
type Knot = [[f64; 2]; 3];

fn corner(x: f64, y: f64) -> Knot {
    [[x, y], [x, y], [x, y]]
}

fn corners(points: &[(f64, f64)]) -> Vec<Knot> {
    points.iter().map(|&(x, y)| corner(x, y)).collect()
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

/// One bake case: the `vmsk` flags (bit 0 inverted), the initial fill
/// record, and each subpath with its operation word (0 exclude, 1 add,
/// 2 subtract, 3 intersect).
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
}

/// A subpath's coverage of the WHOLE canvas through `dr_path_coverage`.
fn whole_canvas(knots: &[Knot]) -> Vec<u16> {
    let anchors: Vec<Value> = knots
        .iter()
        .map(|[i, p, o]| json!({"x": p[0], "y": p[1], "in": i, "out": o}))
        .collect();
    let shape = json!({"fill_rule": "nonzero", "subpaths": [{"closed": true, "anchors": anchors}]});
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

/// One sample by the module doc's formula for `operation`.
fn combine(operation: i16, c: u16, s: u16) -> u16 {
    let (c, s) = (f64::from(c) / 65535.0, f64::from(s) / 65535.0);
    let v = match operation {
        0 => c + s - 2.0 * c * s,
        1 => c + s - c * s,
        2 => c * (1.0 - s),
        3 => c * s,
        _ => unreachable!(),
    };
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// The whole-canvas draw of `case` (module doc).
fn oracle(case: &Case) -> Vec<u16> {
    let full = case.initial == 1 || case.subpaths.first().map(|s| s.1) == Some(2);
    let n = (CANVAS.0 * CANVAS.1) as usize;
    let mut acc = vec![if full { u16::MAX } else { 0 }; n];
    for (knots, operation) in &case.subpaths {
        if knots.len() < 2 {
            continue;
        }
        for (c, s) in acc.iter_mut().zip(whole_canvas(knots)) {
            *c = combine(*operation, *c, s);
        }
    }
    if case.flags & 1 != 0 {
        for c in &mut acc {
            *c = u16::MAX - *c;
        }
    }
    acc
}

/// The fixtures (module doc), every coordinate a multiple of 1/16.
struct Shapes {
    /// A curved closed subpath, four smooth anchors.
    blob: Vec<Knot>,
    triangle: Vec<Knot>,
    /// Overlaps the blob and the triangle.
    big: Vec<Knot>,
    /// Inside the blob.
    small: Vec<Knot>,
    /// Right of the blob, its window disjoint from the blob's.
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
            "disjoint windows",
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
    for (word, op) in [
        (0, "exclude"),
        (1, "add"),
        (2, "subtract"),
        (3, "intersect"),
    ] {
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
fn each_subpath_drawn_over_its_own_window_is_the_whole_canvas_draw() {
    for case in cases() {
        let want = oracle(&case);
        let got = vmsk_bake(&case.body(), CANVAS)
            .expect("the body bakes")
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(got.len(), want.len(), "{}", case.name);
        let differ: Vec<(usize, u16, u16)> = got
            .iter()
            .zip(&want)
            .enumerate()
            .filter(|(_, (g, w))| g != w)
            .map(|(i, (g, w))| (i, *g, *w))
            .take(5)
            .collect();
        assert!(
            differ.is_empty(),
            "{}: (index, bake, oracle) {differ:?}",
            case.name
        );
    }
}

/// An 8-bit open draws the mask a raster record's uncarried `vmsk` holds
/// (a subtracting subpath is not carried) into the layer's mask: the
/// oracle's coverage, each sample rounded to the nearest byte.
#[test]
fn an_eight_bit_open_draws_the_windowed_bake_into_the_layers_mask() {
    let s = shapes();
    let case = Case::new(
        "open",
        0,
        0,
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
    let want: Vec<u8> = oracle(&case)
        .iter()
        .map(|&c| ((u32::from(c) * 255 + 32767) / 65535) as u8)
        .collect();
    let doc = byte_open(&file);
    let mask = unsafe { &*doc }.layers[0]
        .mask
        .as_ref()
        .map(|plane| plane.as_raw().clone());
    unsafe { dr_doc_free(doc) };
    assert_eq!(mask, Some(want));
}
