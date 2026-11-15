//! The Background Eraser (`background_eraser.rs`, `designs/background-eraser.md`) through
//! its three exports, against an oracle written here from the rule alone: its own
//! halved-key colour test on bytes, its own breadth-first walk and its own pixel-mode tip (the
//! overlap of a unit pixel with the dab's square). A pixel-mode dab of odd diameter centred
//! on a pixel covers a square of whole pixels, so every expected coverage is 0 or 1 and
//! every expected alpha exact. The soft tip is checked against `dr_brush_u16` itself: with
//! every pixel admitted the coverage must be the brush's, sample for sample.

use std::collections::VecDeque;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_TRANSPARENCY};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_background_eraser::{
    dr_doc_background_eraser_layer, dr_precision_doc_background_eraser_coverage,
    dr_precision_doc_background_eraser_coverage_plan,
};
use doctored_core::ffi_bitmap::dr_precision_doc_convert_to_bitmap;
use doctored_core::ffi_brush::dr_brush_u16;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_gray::dr_precision_doc_new_gray;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, RgbLayer};
use crate::common::{layer_pixels, take_doc, take_err_string};
use crate::float_fixture::float_doc;

const W: usize = 32;
const H: usize = 24;
const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const PIXEL: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":true}"#;
const SOFT: &str = r#"{"version":1,"hardness":0.4,"roundness":0.7,"angle_degrees":30.0,"pixel":false,"opacity":0.8}"#;

const GREEN: [u8; 3] = [40, 160, 60];
const RED: [u8; 3] = [200, 40, 40];
const BLUE: [u8; 3] = [30, 60, 200];

// ------------------------------------------------------------------ oracle --

#[derive(Clone, Copy, PartialEq)]
enum Sampling {
    Continuous,
    Once,
    Swatch,
}

/// The rule, as the ADR states it.
struct Rule<'a> {
    contiguous: bool,
    sampling: Sampling,
    swatch: Option<[i64; 3]>,
    protect: Option<[i64; 3]>,
    like: &'a dyn Fn([i64; 3], [i64; 3]) -> bool,
}

/// The wand's keys of two byte colours, each within its bound of the other's: R, G and B
/// within the byte tolerance `t`, the halved cyclic differences ⌊(G − R + 255)/2⌋,
/// ⌊(B − G + 255)/2⌋ and ⌊(R − B + 255)/2⌋ within ⌊t/2⌋ (alpha is read as full, so it
/// never differs).
fn halved_keys_within(t: i64) -> impl Fn([i64; 3], [i64; 3]) -> bool {
    move |a, b| {
        let keys = |[r, g, bl]: [i64; 3]| {
            [
                r,
                g,
                bl,
                (g - r + 255).div_euclid(2),
                (bl - g + 255).div_euclid(2),
                (r - bl + 255).div_euclid(2),
            ]
        };
        let bound = [t, t, t, t / 2, t / 2, t / 2];
        keys(a)
            .iter()
            .zip(keys(b))
            .zip(bound)
            .all(|((x, y), m)| (x - y).abs() <= m)
    }
}

/// The overlap of the unit pixel `[p, p + 1)` with `[c − r, c + r]`.
fn overlap(p: f64, c: f64, r: f64) -> f64 {
    ((p + 1.).min(c + r) - p.max(c - r)).clamp(0., 1.)
}

/// The expected field: `codes` is canvas-sized `(rgb, present)`.
fn oracle(codes: &[([i64; 3], bool)], dabs: &[[f64; 4]], rule: &Rule) -> Vec<f64> {
    let (w, h) = (W as i64, H as i64);
    let mut field = vec![0.; codes.len()];
    let mut once: Option<[i64; 3]> = None;
    for d in dabs {
        if d[2] == 0. || d[3] == 0. {
            continue;
        }
        let r = d[2] / 2.;
        let tip = |i: usize| overlap((i % W) as f64, d[0], r) * overlap((i / W) as f64, d[1], r);
        let (hx, hy) = (d[0].floor() as i64, d[1].floor() as i64);
        let hot = (hx >= 0 && hy >= 0 && hx < w && hy < h).then(|| (hy * w + hx) as usize);
        let hot_colour = hot.filter(|&i| codes[i].1).map(|i| codes[i].0);
        let reference = match rule.sampling {
            Sampling::Continuous => hot_colour,
            Sampling::Once => {
                if once.is_none() {
                    once = hot_colour;
                }
                once
            }
            Sampling::Swatch => rule.swatch,
        };
        let Some(reference) = reference else { continue };
        let admit = |i: usize| {
            let (c, present) = codes[i];
            present
                && tip(i) > 0.
                && (rule.like)(reference, c)
                && !rule.protect.is_some_and(|p| (rule.like)(p, c))
        };
        let mut reached = vec![false; codes.len()];
        if rule.contiguous {
            let Some(start) = hot.filter(|&i| admit(i)) else {
                continue;
            };
            let mut queue = VecDeque::from([start]);
            reached[start] = true;
            while let Some(i) = queue.pop_front() {
                let (x, y) = ((i % W) as i64, (i / W) as i64);
                for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let j = (ny * w + nx) as usize;
                    if !reached[j] && admit(j) {
                        reached[j] = true;
                        queue.push_back(j);
                    }
                }
            }
        } else {
            for (i, slot) in reached.iter_mut().enumerate() {
                *slot = admit(i);
            }
        }
        for (i, old) in field.iter_mut().enumerate() {
            if reached[i] {
                let c = tip(i) * d[3];
                *old += c * (1. - *old);
            }
        }
    }
    field
}

// --------------------------------------------------------------- 8 bits --

fn request(tolerance: u32, limits: &str, sampling: &str, extra: Value) -> String {
    let mut v = json!({"version":1,"tolerance":tolerance,"limits":limits,"sampling":sampling});
    if let (Some(map), Value::Object(more)) = (v.as_object_mut(), extra) {
        map.extend(more);
    }
    v.to_string()
}

fn byte_colour(c: [u8; 3]) -> Value {
    json!({"color": [f64::from(c[0]) / 255., f64::from(c[1]) / 255., f64::from(c[2]) / 255., 1.0]})
}

fn scene(paint: impl Fn(usize, usize) -> [u8; 4]) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W as u32, H as u32, |x, y| {
        Rgba(paint(x as usize, y as usize))
    }))
}

fn rgba(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

fn pixels(d: &DrDocument) -> Vec<[u8; 4]> {
    layer_pixels(d, 0)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn erase8(
    d: &DrDocument,
    request: &str,
    brush: &str,
    dabs: &[[f64; 4]],
    selection: Option<&[u8]>,
) -> (Option<DrDocument>, Option<String>) {
    let handle = Box::into_raw(Box::new(d.clone()));
    let request = CString::new(request).unwrap();
    let brush = CString::new(brush).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_background_eraser_layer(
            handle,
            0,
            request.as_ptr(),
            brush.as_ptr(),
            if flat.is_empty() {
                ptr::null()
            } else {
                flat.as_ptr()
            },
            dabs.len(),
            selection.map_or(ptr::null(), <[u8]>::as_ptr),
            selection.map_or(0, <[u8]>::len),
            &mut err,
        )
    };
    unsafe { dr_doc_free(handle) };
    take_doc(out, err)
}

/// The byte layer's R, G and B as the oracle reads them, present when alpha > 0.
fn codes8(d: &DrDocument) -> Vec<([i64; 3], bool)> {
    pixels(d)
        .iter()
        .map(|p| ([0, 1, 2].map(|c| i64::from(p[c])), p[3] > 0))
        .collect()
}

/// Erases `dabs` on `d` and checks every pixel against the oracle: a field
/// of 1 (times a 255 selection byte) clears the alpha, anything else leaves
/// the pixel exactly as it was. Returns the result's pixels.
fn check8(
    d: &DrDocument,
    rule: &Rule,
    request: &str,
    dabs: &[[f64; 4]],
    selection: Option<&[u8]>,
) -> Vec<[u8; 4]> {
    let before = pixels(d);
    let expected = oracle(&codes8(d), dabs, rule);
    assert!(
        expected.iter().all(|&f| f == 0. || f == 1.),
        "a pixel-mode field is 0 or 1"
    );
    let (out, err) = erase8(d, request, PIXEL, dabs, selection);
    assert!(err.is_none(), "{err:?}");
    let after = out.as_ref().map_or_else(|| before.clone(), pixels);
    for (i, (a, b)) in after.iter().zip(&before).enumerate() {
        let gated = expected[i] == 1. && selection.is_none_or(|s| s[i] == 255);
        let want = if gated { [b[0], b[1], b[2], 0] } else { *b };
        assert_eq!(*a, want, "pixel ({}, {}) for {request}", i % W, i / W);
    }
    assert_eq!(
        out.is_none(),
        after == before,
        "None exactly when nothing changes"
    );
    after
}

fn alpha(p: &[[u8; 4]], x: usize, y: usize) -> u8 {
    p[y * W + x][3]
}

fn green_wall_island() -> DrDocument {
    // Green everywhere; a red wall at x = 10, 11 the full height, so the green
    // to its right is cut off from the left inside any dab.
    scene(|x, _| rgba(if (10..12).contains(&x) { RED } else { GREEN }))
}

#[test]
fn limits_reach_the_like_pixels_under_the_tip() {
    let d = green_wall_island();
    let like = halved_keys_within(32);
    let dabs = [[8.5, 12.5, 15., 1.]];
    for contiguous in [true, false] {
        let rule = Rule {
            contiguous,
            sampling: Sampling::Continuous,
            swatch: None,
            protect: None,
            like: &like,
        };
        let limits = if contiguous {
            "contiguous"
        } else {
            "discontiguous"
        };
        let after = check8(
            &d,
            &rule,
            &request(32 * 257, limits, "continuous", json!({})),
            &dabs,
            None,
        );
        assert_eq!(alpha(&after, 5, 12), 0, "the green under the tip clears");
        assert_eq!(alpha(&after, 10, 12), 255, "the red wall stays");
        assert_eq!(alpha(&after, 5, 2), 255, "outside the tip stays");
        // Behind the wall: reached only when the limit is Discontiguous.
        assert_eq!(alpha(&after, 13, 12), if contiguous { 255 } else { 0 });
    }
}

#[test]
fn sampling_takes_each_dab_the_first_dab_or_the_swatch() {
    // Green on the left half, blue on the right; one stroke across both.
    let d = scene(|x, _| rgba(if x < 16 { GREEN } else { BLUE }));
    let like = halved_keys_within(20);
    let dabs: Vec<[f64; 4]> = [4.5, 8.5, 12.5, 16.5, 20.5, 24.5]
        .iter()
        .map(|&x| [x, 12.5, 7., 1.])
        .collect();
    let blue = BLUE.map(i64::from);
    for (sampling, name, swatch) in [
        (Sampling::Continuous, "continuous", None),
        (Sampling::Once, "once", None),
        (Sampling::Swatch, "swatch", Some(blue)),
    ] {
        let rule = Rule {
            contiguous: false,
            sampling,
            swatch,
            protect: None,
            like: &like,
        };
        let extra = if swatch.is_some() {
            json!({"swatch": byte_colour(BLUE)})
        } else {
            json!({})
        };
        let after = check8(
            &d,
            &rule,
            &request(20 * 257, "discontiguous", name, extra),
            &dabs,
            None,
        );
        let (left, right) = (alpha(&after, 5, 12), alpha(&after, 22, 12));
        match sampling {
            Sampling::Continuous => assert_eq!((left, right), (0, 0)),
            Sampling::Once => assert_eq!((left, right), (0, 255)),
            Sampling::Swatch => assert_eq!((left, right), (255, 0)),
        }
    }
}

/// A grey-100 probe: where it sits, its colour, and whether the wand's keys admit it.
type Probe = ((usize, usize), [u8; 3], bool);

#[test]
fn tolerance_is_the_wands_halved_keys() {
    // Grey 100 with, at tolerance 10: (110, 100, 100) inside (R moves 10, q1
    // and q3 move 5 against ⌊10/2⌋), (111, 100, 100) outside, (105, 95, 100)
    // inside, (106, 94, 100) outside although no channel moves more than 6,
    // because q1 = ⌊(G − R + 255)/2⌋ moves 6, and (105, 94, 100) inside
    // although R − G moves 11 (q1 moves 5: G − R + 255 is odd on grey, so the
    // halving rounds the step down). At tolerance 15, (114, 100, 100) is
    // inside and (115, 100, 100), a single-channel step of exactly t, outside
    // (q3 moves 8 against 7). Each probe on its own side of one key's bound;
    // a box of R − G, G − B and R − B within ± t refuses (105, 94, 100) and
    // admits (115, 100, 100).
    let cases: [(u32, &[Probe]); 2] = [
        (
            10,
            &[
                ((4, 12), [110, 100, 100], true),
                ((6, 12), [111, 100, 100], false),
                ((8, 14), [105, 95, 100], true),
                ((10, 12), [106, 94, 100], false),
                ((12, 12), [105, 94, 100], true),
            ],
        ),
        (
            15,
            &[
                ((4, 12), [114, 100, 100], true),
                ((6, 12), [115, 100, 100], false),
            ],
        ),
    ];
    for (t, probes) in cases {
        let d = scene(|x, y| {
            let colour = probes
                .iter()
                .find(|(at, ..)| *at == (x, y))
                .map_or([100, 100, 100], |p| p.1);
            rgba(colour)
        });
        let like = halved_keys_within(i64::from(t));
        let rule = Rule {
            contiguous: false,
            sampling: Sampling::Continuous,
            swatch: None,
            protect: None,
            like: &like,
        };
        let after = check8(
            &d,
            &rule,
            &request(t * 257, "discontiguous", "continuous", json!({})),
            &[[7.5, 12.5, 13., 1.]],
            None,
        );
        for &((x, y), _, inside) in probes {
            assert_eq!(
                alpha(&after, x, y),
                if inside { 0 } else { 255 },
                "t {t} ({x}, {y})"
            );
        }
    }
}

#[test]
fn protect_keeps_like_pixels_and_stops_the_walk() {
    // Grey 100 with a column of (109, 100, 100) at x = 10, 11: like the grey at
    // tolerance 10, and like the protected (118, 100, 100), which the grey is not.
    let near = [109, 100, 100];
    let d = scene(|x, _| {
        rgba(if (10..12).contains(&x) {
            near
        } else {
            [100, 100, 100]
        })
    });
    let like = halved_keys_within(10);
    let protect = [118, 100, 100];
    let codes = |c: [u8; 3]| c.map(i64::from);
    let dab = [[6.5, 12.5, 17., 1.]];
    let run = |contiguous: bool, protect: Option<[u8; 3]>, dabs: &[[f64; 4]]| {
        let rule = Rule {
            contiguous,
            sampling: Sampling::Continuous,
            swatch: None,
            protect: protect.map(codes),
            like: &like,
        };
        let limits = if contiguous {
            "contiguous"
        } else {
            "discontiguous"
        };
        let extra = protect.map_or(json!({}), |p| json!({"protect": byte_colour(p)}));
        check8(
            &d,
            &rule,
            &request(10 * 257, limits, "continuous", extra),
            dabs,
            None,
        )
    };
    // Unprotected, the near column is like the grey: erased, and the walk passes it.
    let open = run(true, None, &dab);
    assert_eq!((alpha(&open, 10, 12), alpha(&open, 13, 12)), (0, 0));
    // Protected: the column stays and stops the walk.
    let kept = run(true, Some(protect), &dab);
    assert_eq!(
        (
            alpha(&kept, 5, 12),
            alpha(&kept, 10, 12),
            alpha(&kept, 13, 12)
        ),
        (0, 255, 255)
    );
    // Discontiguous reaches past it.
    let past = run(false, Some(protect), &dab);
    assert_eq!((alpha(&past, 10, 12), alpha(&past, 13, 12)), (255, 0));
    // A hot spot ON a protected pixel: Contiguous erases nothing at all, not
    // even the hot spot; Discontiguous still clears the grey around it.
    let on = [[10.5, 12.5, 9., 1.]];
    let none = run(true, Some(protect), &on);
    assert!(none.iter().all(|p| p[3] == 255));
    let around = run(false, Some(protect), &on);
    assert_eq!((alpha(&around, 8, 12), alpha(&around, 10, 12)), (0, 255));
}

#[test]
fn transparency_hot_spots_and_the_canvas_edge() {
    // Green with a fully transparent pixel at (8, 12) and a half-erased green
    // one at (5, 12).
    let d = scene(|x, y| match (x, y) {
        (8, 12) => [GREEN[0], GREEN[1], GREEN[2], 0],
        (5, 12) => [GREEN[0], GREEN[1], GREEN[2], 128],
        _ => rgba(GREEN),
    });
    let like = halved_keys_within(16);
    let rule = |contiguous: bool, sampling: Sampling, swatch: Option<[i64; 3]>| Rule {
        contiguous,
        sampling,
        swatch,
        protect: None,
        like: &like,
    };
    let green = GREEN.map(i64::from);
    // A Continuous dab whose hot spot is transparent erases nothing.
    let none = erase8(
        &d,
        &request(16 * 257, "discontiguous", "continuous", json!({})),
        PIXEL,
        &[[8.5, 12.5, 7., 1.]],
        None,
    );
    assert!(none.0.is_none() && none.1.is_none());
    // The half-erased green is still green: cleared with its neighbours.
    let half = check8(
        &d,
        &rule(false, Sampling::Continuous, None),
        &request(16 * 257, "discontiguous", "continuous", json!({})),
        &[[4.5, 12.5, 5., 1.]],
        None,
    );
    assert_eq!((alpha(&half, 5, 12), alpha(&half, 3, 12)), (0, 0));
    // Once: the first dab's hot spot is transparent, so the second's green is
    // the stroke's reference.
    check8(
        &d,
        &rule(false, Sampling::Once, None),
        &request(16 * 257, "discontiguous", "once", json!({})),
        &[
            [8.5, 12.5, 3., 1.],
            [16.5, 12.5, 5., 1.],
            [24.5, 12.5, 5., 1.],
        ],
        None,
    );
    // A hot spot off the canvas: Continuous has no reference; a swatch erases
    // Discontiguous, and Contiguous still needs the hot spot.
    let off = [[-1.5, 12.5, 9., 1.]];
    let none = erase8(
        &d,
        &request(16 * 257, "discontiguous", "continuous", json!({})),
        PIXEL,
        &off,
        None,
    );
    assert!(none.0.is_none() && none.1.is_none());
    let swatch = json!({"swatch": byte_colour(GREEN)});
    let edge = check8(
        &d,
        &rule(false, Sampling::Swatch, Some(green)),
        &request(16 * 257, "discontiguous", "swatch", swatch.clone()),
        &off,
        None,
    );
    assert_eq!(alpha(&edge, 1, 12), 0);
    let none = erase8(
        &d,
        &request(16 * 257, "contiguous", "swatch", swatch),
        PIXEL,
        &off,
        None,
    );
    assert!(none.0.is_none() && none.1.is_none());
}

#[test]
fn the_selection_gates_the_erase() {
    let d = scene(|_, _| rgba(GREEN));
    let like = halved_keys_within(0);
    let rule = Rule {
        contiguous: true,
        sampling: Sampling::Continuous,
        swatch: None,
        protect: None,
        like: &like,
    };
    let selection: Vec<u8> = (0..W * H)
        .map(|i| if i % W < 8 { 255 } else { 0 })
        .collect();
    let after = check8(
        &d,
        &rule,
        &request(0, "contiguous", "continuous", json!({})),
        &[[8.5, 12.5, 11., 1.]],
        Some(&selection),
    );
    assert_eq!((alpha(&after, 5, 12), alpha(&after, 10, 12)), (0, 255));
}

#[test]
fn nothing_to_erase_answers_none_and_locks_refuse() {
    let d = green_wall_island();
    // The swatch is blue, which nothing under the tip is.
    let blue = request(
        0,
        "discontiguous",
        "swatch",
        json!({"swatch": byte_colour(BLUE)}),
    );
    let (out, err) = erase8(&d, &blue, PIXEL, &[[8.5, 12.5, 9., 1.]], None);
    assert!(out.is_none() && err.is_none());
    let green = request(0, "discontiguous", "continuous", json!({}));
    for lock in [LOCK_PIXELS, LOCK_TRANSPARENCY] {
        let locked = d.with_layer_locks(0, lock).unwrap();
        let (out, err) = erase8(&locked, &green, PIXEL, &[[4.5, 12.5, 5., 1.]], None);
        assert!(out.is_none() && err.is_none(), "lock {lock}");
    }
}

// --------------------------------------------------------- typed documents --

fn rgb16_doc(paint: impl Fn(usize, usize) -> [u16; 4]) -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = (0..W * H).map(|i| paint(i % W, i / W)).collect();
    let image =
        PrecisionImage::from_rgba16(W as u32, H as u32, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W as u32,
            H as u32,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .unwrap(),
    ))
}

fn coverage(
    doc: *const PrecisionDocument,
    request: &str,
    brush: &str,
    dabs: &[[f64; 4]],
) -> Result<Vec<u16>, String> {
    let request = CString::new(request).unwrap();
    let brush = CString::new(brush).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut out = vec![7u16; W * H];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_background_eraser_coverage(
            doc,
            0,
            request.as_ptr(),
            brush.as_ptr(),
            flat.as_ptr(),
            dabs.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(out)
    } else {
        assert!(
            out.iter().all(|&v| v == 7),
            "a refusal leaves the output unchanged"
        );
        Err(take_err_string(err))
    }
}

fn plan(doc: *const PrecisionDocument, request: &str, dabs: &[[f64; 4]]) -> Result<Value, String> {
    let request = CString::new(request).unwrap();
    let brush = CString::new(PIXEL).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_background_eraser_coverage_plan(
            doc,
            0,
            request.as_ptr(),
            brush.as_ptr(),
            flat.as_ptr(),
            dabs.len(),
            LIMIT,
            0,
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

/// The plane against the oracle's field, every sample exactly.
fn assert_plane(plane: &[u16], expected: &[f64], what: &str) {
    for (i, (&got, &f)) in plane.iter().zip(expected).enumerate() {
        assert_eq!(
            got,
            (f * 65535.).round() as u16,
            "{what}: ({}, {})",
            i % W,
            i / W
        );
    }
}

fn codes16(doc: *const PrecisionDocument) -> Vec<([i64; 3], bool)> {
    let doc = unsafe { &*doc };
    let image = doc.layer(0).unwrap().source().unwrap();
    image
        .read_rect_rgba16(0, 0, W as u32, H as u32)
        .unwrap()
        .iter()
        .map(|p| ([0, 1, 2].map(|c| i64::from(p[c])), p[3] > 0))
        .collect()
}

/// The layer's R, G and B as the bytes the predicate compares at 16 bits: each
/// code's nearest byte, (v + 128) / 257 (`selection_similar`'s **Resolution**).
fn bytes16(doc: *const PrecisionDocument) -> Vec<([i64; 3], bool)> {
    codes16(doc)
        .into_iter()
        .map(|(c, present)| (c.map(|v| (v + 128) / 257), present))
        .collect()
}

#[test]
fn sixteen_bits_compare_the_nearest_bytes() {
    // A field, a wall unlike it, the field again behind the wall, and two near
    // colours one code apart across a byte edge: R 21202 is byte 82, four from
    // the field's 78 and inside tolerance 4 × 257, and 21203 is byte 83,
    // outside. Read as u16 codes both would be outside (21202 − 20000 > 1028),
    // so the first pins the byte reading.
    let field = [20000, 30000, 40000, 65535];
    let doc = rgb16_doc(|x, y| match (x, y) {
        (10 | 11, _) => [60000, 1000, 1000, 65535],
        (4, 12) => [21202, 31000, 41000, 65535],
        (6, 12) => [21203, 31000, 41000, 65535],
        _ => field,
    });
    let like = halved_keys_within(4);
    let dabs = [[7.5, 12.5, 15., 1.]];
    let codes = bytes16(doc);
    for (contiguous, limits) in [(true, "contiguous"), (false, "discontiguous")] {
        for (sampling, name) in [
            (Sampling::Continuous, "continuous"),
            (Sampling::Swatch, "swatch"),
        ] {
            let swatch = (sampling == Sampling::Swatch).then_some([78, 117, 156]);
            let rule = Rule {
                contiguous,
                sampling,
                swatch,
                protect: None,
                like: &like,
            };
            let extra = if swatch.is_some() {
                json!({"swatch":{"color":[20000. / 65535., 30000. / 65535., 40000. / 65535., 1.],"color_space":"document"}})
            } else {
                json!({})
            };
            let plane =
                coverage(doc, &request(4 * 257, limits, name, extra), PIXEL, &dabs).unwrap();
            assert_plane(
                &plane,
                &oracle(&codes, &dabs, &rule),
                &format!("{limits} {name}"),
            );
            assert_eq!(
                plane[12 * W + 4],
                65535,
                "inside on bytes, outside on codes"
            );
            assert_eq!(plane[12 * W + 6], 0, "one code more crosses the byte edge");
            assert_eq!(plane[12 * W + 13] == 0, contiguous, "behind the wall");
        }
    }
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn with_every_pixel_admitted_the_coverage_is_the_brushs() {
    let doc = rgb16_doc(|_, _| [12345, 23456, 34567, 65535]);
    let dabs = [
        [6.3, 7.1, 9.5, 0.7],
        [11.8, 9.4, 9.5, 0.4],
        [17.2, 12.6, 12., 1.],
        [22.9, 15.3, 7., 0.55],
    ];
    let plane = coverage(
        doc,
        &request(65535, "discontiguous", "continuous", json!({})),
        SOFT,
        &dabs,
    )
    .unwrap();
    let brush = CString::new(SOFT).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut expected = vec![0u16; W * H];
    let mut err = ptr::null_mut();
    assert!(unsafe {
        dr_brush_u16(
            W as u32,
            H as u32,
            brush.as_ptr(),
            flat.as_ptr(),
            dabs.len(),
            expected.as_mut_ptr(),
            expected.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    });
    assert!(
        expected.iter().any(|&v| v > 0 && v < 65535),
        "a soft stroke has partial coverage"
    );
    assert_eq!(plane, expected);
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn a_float_layer_compares_its_display_codes() {
    let red = [1.0f32, 0., 0., 1.];
    let blue = [0.0f32, 0., 1., 1.];
    let records: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            if (10..12).contains(&(i % W)) {
                blue
            } else {
                red
            }
        })
        .collect();
    let doc = Box::into_raw(Box::new(float_doc(
        W as u32,
        H as u32,
        &IccProfile::srgb(),
        &[records],
    )));
    // The display codes' bytes: pure blue and pure red.
    let codes: Vec<([i64; 3], bool)> = (0..W * H)
        .map(|i| {
            (
                if (10..12).contains(&(i % W)) {
                    [0, 0, 255]
                } else {
                    [255, 0, 0]
                },
                true,
            )
        })
        .collect();
    let like = halved_keys_within(0);
    let dabs = [[7.5, 12.5, 15., 1.]];
    for (contiguous, limits) in [(true, "contiguous"), (false, "discontiguous")] {
        let rule = Rule {
            contiguous,
            sampling: Sampling::Continuous,
            swatch: None,
            protect: None,
            like: &like,
        };
        let plane = coverage(
            doc,
            &request(0, limits, "continuous", json!({})),
            PIXEL,
            &dabs,
        )
        .unwrap();
        assert_plane(&plane, &oracle(&codes, &dabs, &rule), limits);
    }
    // A linear blue swatch reaches the blue wall's display codes.
    let rule = Rule {
        contiguous: false,
        sampling: Sampling::Swatch,
        swatch: Some([0, 0, 255]),
        protect: None,
        like: &like,
    };
    let extra = json!({"swatch":{"color":[0.,0.,1.,1.],"color_space":"linear"}});
    let plane = coverage(
        doc,
        &request(0, "discontiguous", "swatch", extra),
        PIXEL,
        &dabs,
    )
    .unwrap();
    assert_plane(&plane, &oracle(&codes, &dabs, &rule), "swatch");
    assert_eq!((plane[12 * W + 10], plane[12 * W + 5]), (65535, 0));
    unsafe { dr_precision_doc_free(doc) };
}

/// Green, a red wall at x = 10, 11, green behind it, as u16 RGB.
fn wall16(x: usize) -> [u16; 4] {
    let c = if (10..12).contains(&x) { RED } else { GREEN };
    [c[0], c[1], c[2], 255].map(|v| u16::from(v) * 257)
}

#[test]
fn lab_and_cmyk_layers_compare_their_own_codes() {
    let dabs = [[7.5, 12.5, 15., 1.]];
    let equal = |a: [i64; 3], b: [i64; 3]| a == b;
    // Lab (8-bit): the stored codes; at tolerance 0 the wand's key admits
    // exactly the equal codes of these well-separated colours.
    let rgb = rgb16_doc(|x, _| wall16(x));
    let options = CString::new(r#"{"depth":8}"#).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "{}", take_err_string(err));
    unsafe { dr_precision_doc_free(rgb) };
    let codes = codes16(lab);
    for (contiguous, limits) in [(true, "contiguous"), (false, "discontiguous")] {
        let rule = Rule {
            contiguous,
            sampling: Sampling::Continuous,
            swatch: None,
            protect: None,
            like: &equal,
        };
        let plane = coverage(
            lab,
            &request(0, limits, "continuous", json!({})),
            PIXEL,
            &dabs,
        )
        .unwrap();
        assert_plane(
            &plane,
            &oracle(&codes, &dabs, &rule),
            &format!("lab {limits}"),
        );
        assert_eq!(plane[12 * W + 13] == 0, contiguous);
    }
    unsafe { dr_precision_doc_free(lab) };
    // CMYK: the stored ink; at tolerance 0 equal ink is admitted.
    let cmyk = cmyk_layers(
        W as u32,
        H as u32,
        vec![RgbLayer {
            name: "Scene",
            w: W as u32,
            h: H as u32,
            offset: (0, 0),
            pixels: (0..W * H).map(|i| wall16(i % W)).collect(),
        }],
    );
    let ink = unsafe { &*cmyk }
        .layer(0)
        .unwrap()
        .cmyk_source()
        .unwrap()
        .to_contiguous()
        .unwrap();
    // All four plates in the oracle's three slots: equal triples are equal ink.
    let codes: Vec<([i64; 3], bool)> = ink
        .iter()
        .map(|p| {
            let [c, m, y, k, a] = p.map(i64::from);
            ([c, m, y * 65536 + k], a > 0)
        })
        .collect();
    for (contiguous, limits) in [(true, "contiguous"), (false, "discontiguous")] {
        let rule = Rule {
            contiguous,
            sampling: Sampling::Continuous,
            swatch: None,
            protect: None,
            like: &equal,
        };
        let plane = coverage(
            cmyk,
            &request(0, limits, "continuous", json!({})),
            PIXEL,
            &dabs,
        )
        .unwrap();
        assert_plane(
            &plane,
            &oracle(&codes, &dabs, &rule),
            &format!("cmyk {limits}"),
        );
    }
    // A swatch of the wall's own ink clears the wall.
    let wall = ink[12 * W + 10].map(|v| f64::from(v) / 65535.);
    let extra = json!({"swatch":{"ink": wall}});
    let plane = coverage(
        cmyk,
        &request(0, "discontiguous", "swatch", extra),
        PIXEL,
        &dabs,
    )
    .unwrap();
    assert_eq!((plane[12 * W + 10], plane[12 * W + 5]), (65535, 0));
    // An RGB colour on a CMYK document refuses by name.
    let rgb_swatch = json!({"swatch":{"color":[1.,0.,0.,1.],"color_space":"srgb"}});
    let refused = coverage(
        cmyk,
        &request(0, "discontiguous", "swatch", rgb_swatch),
        PIXEL,
        &dabs,
    );
    assert!(refused
        .unwrap_err()
        .contains("CMYK document's background eraser colours are ink"));
    unsafe { dr_precision_doc_free(cmyk) };
}

fn new_gray() -> *mut PrecisionDocument {
    let request =
        CString::new(r#"{"depth":8,"space":"gray_gamma_22","fill_srgb":[0.5,0.5,0.5,1]}"#).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_gray(
            W as u32,
            H as u32,
            ptr::null(),
            0,
            request.as_ptr(),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

#[test]
fn a_gray_layer_erases_and_a_bitmap_document_refuses() {
    // A uniform grey layer: every pixel is like the hot spot, so the plane is
    // the dab's square.
    let gray = new_gray();
    let dabs = [[7.5, 12.5, 5., 1.]];
    let plane = coverage(
        gray,
        &request(0, "contiguous", "continuous", json!({})),
        PIXEL,
        &dabs,
    )
    .unwrap();
    for (i, &v) in plane.iter().enumerate() {
        let inside = (5..10).contains(&(i % W)) && (10..15).contains(&(i / W));
        assert_eq!(v, if inside { 65535 } else { 0 }, "({}, {})", i % W, i / W);
    }
    let options = CString::new(r#"{"method":"threshold"}"#).unwrap();
    let mut err = ptr::null_mut();
    let bitmap = unsafe {
        dr_precision_doc_convert_to_bitmap(gray, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!bitmap.is_null(), "{}", take_err_string(err));
    let text = request(0, "contiguous", "continuous", json!({}));
    let refusal = "background_eraser is unavailable in a Bitmap document";
    assert!(plan(bitmap, &text, &dabs).unwrap_err().starts_with(refusal));
    assert!(coverage(bitmap, &text, PIXEL, &dabs)
        .unwrap_err()
        .starts_with(refusal));
    unsafe {
        dr_precision_doc_free(gray);
        dr_precision_doc_free(bitmap);
    }
}

#[test]
fn requests_and_the_work_bound_refuse_by_name() {
    let d = green_wall_island();
    let dab = [[8.5, 12.5, 9., 1.]];
    let refused = |text: &str| erase8(&d, text, PIXEL, &dab, None).1.unwrap_or_default();
    assert!(
        refused(&request(0, "contiguous", "continuous", json!({"extra": 1})))
            .starts_with("invalid background eraser request")
    );
    assert!(refused(&request(0, "edges", "continuous", json!({})))
        .starts_with("invalid background eraser request"));
    assert_eq!(
        refused(&request(
            0,
            "contiguous",
            "continuous",
            json!({"version": 2})
        )),
        "background eraser request requires version 1"
    );
    assert_eq!(
        refused(&request(0, "contiguous", "swatch", json!({}))),
        "background eraser swatch sampling requires a swatch colour"
    );
    assert_eq!(
        refused(&request(
            0,
            "contiguous",
            "once",
            json!({"swatch": byte_colour(GREEN)})
        )),
        "background eraser takes a swatch colour only with swatch sampling"
    );
    let srgb = json!({"protect":{"color":[0.,0.,0.,1.],"color_space":"srgb"}});
    assert!(refused(&request(0, "contiguous", "continuous", srgb))
        .starts_with("a byte document's background eraser colours are document-space"));
    let short = [255u8; 4];
    let (out, err) = erase8(
        &d,
        &request(0, "contiguous", "continuous", json!({})),
        PIXEL,
        &dab,
        Some(&short),
    );
    assert!(out.is_none());
    assert_eq!(
        err.as_deref(),
        Some("background eraser selection length differs from the canvas")
    );
    // 6000 dabs of a 1000 px hard brush, each box the whole 32 × 24 canvas:
    // 64 + 1 units a pixel a dab is 299.5 million, past the bound (and far
    // inside the brush's own 2 billion).
    let big = vec![[16., 12., 1000., 1.]; 6000];
    let hard = r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;
    let bound =
        "background eraser exceeds 256 million work units; use a shorter stroke or a smaller brush";
    let (out, err) = erase8(
        &d,
        &request(0, "contiguous", "continuous", json!({})),
        hard,
        &big,
        None,
    );
    assert!(out.is_none());
    assert_eq!(err.as_deref(), Some(bound));
    let doc = rgb16_doc(|x, _| wall16(x));
    assert_eq!(
        plan(
            doc,
            &request(0, "contiguous", "continuous", json!({})),
            &big[..1]
        )
        .map(|_| ()),
        Ok(())
    );
    let text = CString::new(request(0, "contiguous", "continuous", json!({}))).unwrap();
    let brush = CString::new(hard).unwrap();
    let flat: Vec<f64> = big.iter().flatten().copied().collect();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_background_eraser_coverage_plan(
            doc,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            flat.as_ptr(),
            big.len(),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), bound);
    // A layer past the end refuses on a typed document.
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_background_eraser_coverage_plan(
            doc,
            3,
            text.as_ptr(),
            brush.as_ptr(),
            flat.as_ptr(),
            1,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "background eraser target is out of range"
    );
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn null_inputs_answer_null_or_false() {
    let d = Box::into_raw(Box::new(green_wall_island()));
    let text = CString::new(request(0, "contiguous", "continuous", json!({}))).unwrap();
    let brush = CString::new(PIXEL).unwrap();
    let dab = [8.5, 12.5, 9., 1.];
    let doc = rgb16_doc(|x, _| wall16(x));
    let mut out = vec![0u16; W * H];
    unsafe {
        // A NULL document is NULL with no error; a NULL request names itself.
        let mut err = ptr::null_mut();
        assert!(dr_doc_background_eraser_layer(
            ptr::null(),
            0,
            text.as_ptr(),
            brush.as_ptr(),
            dab.as_ptr(),
            1,
            ptr::null(),
            0,
            &mut err
        )
        .is_null());
        assert!(err.is_null());
        assert!(dr_doc_background_eraser_layer(
            d,
            0,
            ptr::null(),
            brush.as_ptr(),
            dab.as_ptr(),
            1,
            ptr::null(),
            0,
            &mut err
        )
        .is_null());
        assert_eq!(take_err_string(err), "background eraser request is NULL");
        let mut err = ptr::null_mut();
        assert!(dr_doc_background_eraser_layer(
            d,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            ptr::null(),
            1,
            ptr::null(),
            0,
            &mut err
        )
        .is_null());
        assert!(take_err_string(err).starts_with("native brush requires a valid array"));
        // An out-of-range layer is NULL with no error.
        let mut err = ptr::null_mut();
        assert!(dr_doc_background_eraser_layer(
            d,
            9,
            text.as_ptr(),
            brush.as_ptr(),
            dab.as_ptr(),
            1,
            ptr::null(),
            0,
            &mut err
        )
        .is_null());
        assert!(err.is_null());
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_background_eraser_coverage_plan(
            ptr::null(),
            0,
            text.as_ptr(),
            brush.as_ptr(),
            dab.as_ptr(),
            1,
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert_eq!(take_err_string(err), "required handle is NULL");
        for (document, output) in [
            (ptr::null(), out.as_mut_ptr()),
            (doc as *const _, ptr::null_mut()),
        ] {
            let mut err = ptr::null_mut();
            assert!(!dr_precision_doc_background_eraser_coverage(
                document,
                0,
                text.as_ptr(),
                brush.as_ptr(),
                dab.as_ptr(),
                1,
                output,
                W * H,
                LIMIT,
                0,
                ptr::null(),
                &mut err
            ));
            assert!(!err.is_null());
            take_err_string(err);
        }
        dr_doc_free(d);
        dr_precision_doc_free(doc);
    }
}
