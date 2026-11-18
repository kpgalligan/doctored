//! HELD OPERATIONS through `dr_artwork_path_coverage` (a path SHAPE layer's
//! fill, `designs/v1-paths-held-subpaths.md`, HS-3): one operation code per
//! move verb (0 unite, 1 subtract, 2 intersect, 3 exclude) folds each
//! subpath's own region in order.
//!
//! The oracle is arithmetic: two axis-aligned rectangles with fractional
//! sides, whose per-pixel areas are the products of their interval overlaps
//! with the pixel, and whose common area is the overlap of the rectangle the
//! two intervals' intersections make. Union is `a + b − ab`, difference
//! `a − ab`, intersection `ab`, symmetric difference `a + b − 2ab`. A
//! rectangle's own winding is ±1, so each operation's area is the same under
//! nonzero and even-odd. Bound: within 1 of 65535 per sample.
//!
//! EDGELESS SUBPATHS. A move that draws no edge (a bare move directly
//! followed by the next move, a move and a close, a move and a line to the
//! same point) is still a subpath and takes a code: it is outside everywhere,
//! so a unite, subtract or exclude of it changes nothing (sample for sample
//! against the call without it) and an intersect with it clears every region
//! before it.
//!
//! Every refusal leaves a sentinel-filled output untouched. Bounds declared
//! before measuring.

use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::dr_artwork_path_coverage;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SENTINEL: u16 = 0x4a4a;
/// The window every case draws.
const SIZE: (u32, u32) = (16, 14);
/// How far a held-operations sample may sit from its exact area, in codes.
const EXACT: u16 = 1;

/// The rectangles `[x0, y0, x1, y1]`: each side fractional, overlapping.
const A: [f64; 4] = [1.25, 2.75, 9.5, 8.625];
const B: [f64; 4] = [5.375, 4.5, 13.75, 11.25];
/// The point an edgeless subpath sits on, inside `A` and `B` both.
const DOT: [f64; 2] = [7.5, 6.25];

const UNITE: u8 = 0;
const SUBTRACT: u8 = 1;
const INTERSECT: u8 = 2;
const EXCLUDE: u8 = 3;
const OPERATIONS: [u8; 4] = [UNITE, SUBTRACT, INTERSECT, EXCLUDE];

/// A path's verbs and points.
#[derive(Clone, Default)]
struct Outline {
    verbs: Vec<u8>,
    points: Vec<[f64; 2]>,
}

impl Outline {
    /// Appends the rectangle `r` as one closed subpath.
    fn rectangle(mut self, r: [f64; 4]) -> Self {
        self.verbs.extend([0, 1, 1, 1, 4]);
        self.points
            .extend([[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]);
        self
    }

    /// Appends a subpath at `DOT` that draws no edge, in one of three forms:
    /// 0 a bare move, 1 a move and a close, 2 a move and a line to the point.
    fn edgeless(mut self, form: u8) -> Self {
        self.verbs.push(0);
        self.points.push(DOT);
        match form {
            0 => {}
            1 => self.verbs.push(4),
            _ => {
                self.verbs.push(1);
                self.points.push(DOT);
            }
        }
        self
    }
}

unsafe fn message(e: *mut c_char) -> String {
    if e.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(e) };
    s
}

/// The export's plane of `outline` with `ops` and `op_count` passed as given,
/// into a sentinel-filled output; on a refusal the output must be untouched.
fn raw(
    outline: &Outline,
    even_odd: bool,
    ops: *const u8,
    op_count: usize,
) -> Result<Vec<u16>, String> {
    let mut out = vec![SENTINEL; (SIZE.0 * SIZE.1) as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_artwork_path_coverage(
            outline.verbs.as_ptr(),
            outline.verbs.len(),
            outline.points.as_ptr().cast(),
            outline.points.len(),
            SIZE.0,
            SIZE.1,
            even_odd,
            ops,
            op_count,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    let m = unsafe { message(e) };
    if ok {
        assert!(m.is_empty(), "an accepted call reported {m}");
        Ok(out)
    } else {
        assert!(
            out.iter().all(|&v| v == SENTINEL),
            "a refusal ({m}) wrote the output"
        );
        Err(m)
    }
}

/// The plane of `outline` with `ops` (none when empty, as a NULL pointer).
fn plane(outline: &Outline, even_odd: bool, ops: &[u8]) -> Result<Vec<u16>, String> {
    let pointer = if ops.is_empty() {
        ptr::null()
    } else {
        ops.as_ptr()
    };
    raw(outline, even_odd, pointer, ops.len())
}

/// The length of `[low, high]` inside the unit interval from `at`.
fn overlap(low: f64, high: f64, at: f64) -> f64 {
    (high.min(at + 1.0) - low.max(at)).max(0.0)
}

/// The area of rectangle `r` inside pixel `(px, py)`.
fn area(r: [f64; 4], px: f64, py: f64) -> f64 {
    overlap(r[0], r[2], px) * overlap(r[1], r[3], py)
}

/// The exact plane of `A` folded with `B` by `op` (`A` first, as a unite).
fn exact(op: u8) -> Vec<u16> {
    let common = [
        A[0].max(B[0]),
        A[1].max(B[1]),
        A[2].min(B[2]),
        A[3].min(B[3]),
    ];
    (0..SIZE.1)
        .flat_map(|y| (0..SIZE.0).map(move |x| (f64::from(x), f64::from(y))))
        .map(|(px, py)| {
            let (a, b) = (area(A, px, py), area(B, px, py));
            let ab = if common[0] < common[2] && common[1] < common[3] {
                area(common, px, py)
            } else {
                0.0
            };
            let covered = match op {
                UNITE => a + b - ab,
                SUBTRACT => a - ab,
                INTERSECT => ab,
                _ => a + b - 2.0 * ab,
            };
            (covered * 65535.0).round() as u16
        })
        .collect()
}

/// The exact plane of `B` alone.
fn exact_b() -> Vec<u16> {
    (0..SIZE.1)
        .flat_map(|y| (0..SIZE.0).map(move |x| (f64::from(x), f64::from(y))))
        .map(|(px, py)| (area(B, px, py) * 65535.0).round() as u16)
        .collect()
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

fn two() -> Outline {
    Outline::default().rectangle(A).rectangle(B)
}

#[test]
fn each_held_operation_on_two_rectangles_covers_its_exact_area() {
    for even_odd in [false, true] {
        for op in OPERATIONS {
            let held = plane(&two(), even_odd, &[UNITE, op]).expect("held");
            let (most, at) = worst(&held, &exact(op));
            println!("op {op}, even_odd {even_odd}: worst difference {most} at sample {at}");
            assert!(
                most <= EXACT,
                "op {op}, even_odd {even_odd}: sample {at} differs from the exact area by {most}"
            );
        }
    }
}

#[test]
fn no_operations_fill_the_summed_winding_as_before() {
    let codes = [SUBTRACT, INTERSECT];
    for even_odd in [false, true] {
        let without = plane(&two(), even_odd, &[]).expect("no ops");
        let ignored = raw(&two(), even_odd, codes.as_ptr(), 0).expect("count 0");
        assert_eq!(
            without, ignored,
            "even_odd {even_odd}: a zero count read ops"
        );
        // The summed winding: nonzero fills the union, even-odd the overlap's
        // complement.
        let oracle = exact(if even_odd { EXCLUDE } else { UNITE });
        let (most, at) = worst(&without, &oracle);
        assert!(
            most <= EXACT,
            "even_odd {even_odd}: sample {at} differs from the summed winding's area by {most}"
        );
    }
}

#[test]
fn held_operation_refusals_leave_the_output_untouched() {
    assert_eq!(
        raw(&two(), false, ptr::null(), 2).expect_err("NULL ops with a count"),
        "invalid native artwork path buffers"
    );
    let many = vec![UNITE; 4097];
    assert_eq!(
        plane(&two(), false, &many).expect_err("too many codes"),
        "too many subpaths in a path (max 4096)"
    );
    assert_eq!(
        plane(&two(), false, &[UNITE, 4]).expect_err("an unknown code"),
        "invalid native artwork path operation"
    );
    assert_eq!(
        plane(&two(), false, &[UNITE, SUBTRACT, UNITE]).expect_err("three codes, two moves"),
        "artwork path operations do not match its subpaths"
    );
    assert_eq!(
        plane(&two(), false, &[UNITE]).expect_err("one code, two moves"),
        "artwork path operations do not match its subpaths"
    );
}

#[test]
fn an_edgeless_subpath_takes_a_code_and_has_no_region() {
    let nothing = vec![0u16; (SIZE.0 * SIZE.1) as usize];
    for form in 0..3u8 {
        let three = Outline::default().rectangle(A).edgeless(form).rectangle(B);
        for even_odd in [false, true] {
            for last in OPERATIONS {
                let without = plane(&two(), even_odd, &[UNITE, last]).expect("two");
                for middle in [UNITE, SUBTRACT, EXCLUDE] {
                    let held = plane(&three, even_odd, &[UNITE, middle, last]).expect("three");
                    assert_eq!(
                        held, without,
                        "form {form}, even_odd {even_odd}: [unite, {middle}, {last}] is not the call without the edgeless subpath"
                    );
                }
                // An intersect with nothing clears A; B then folds onto
                // nothing.
                let held = plane(&three, even_odd, &[UNITE, INTERSECT, last]).expect("intersect");
                let oracle = match last {
                    UNITE | EXCLUDE => exact_b(),
                    _ => nothing.clone(),
                };
                let (most, at) = worst(&held, &oracle);
                assert!(
                    most <= EXACT,
                    "form {form}, even_odd {even_odd}: [unite, intersect, {last}] sample {at} differs from B folded onto nothing by {most}"
                );
            }
            assert_eq!(
                plane(&three, even_odd, &[UNITE, UNITE]).expect_err("two codes, three moves"),
                "artwork path operations do not match its subpaths",
                "form {form}"
            );
        }
    }
}
