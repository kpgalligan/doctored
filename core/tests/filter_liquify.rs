//! Liquify: the `liquify` op of `dr_image_filter_op` at 8 bits, the
//! selected edit at 16, and the workspace's field handle
//! (`dr_liquify_field_*`). Every expectation is stated from the documented
//! model, not read back from the kernel: a full-strength push is a whole-pixel
//! translation, a half push is a Catmull-Rom resample written here, a frozen
//! disc keeps its bytes, each tool moves a blob the way its name says, and a
//! stroke applied whole equals the same stroke applied a segment at a time.
//! Planes are at most 2048 px a side, so the field has one node per pixel,
//! except in `a_plane_over_2048_px_pushes_through_a_coarser_grid_and_its_proxy`
//! (2100 px, a node every 2 px).
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::{dr_image_filter_op, dr_image_filter_op_scratch_bytes};
use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_liquify::{
    dr_liquify_field_apply, dr_liquify_field_create, dr_liquify_field_free,
    dr_liquify_field_freeze, dr_liquify_field_render,
};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::liquify::LiquifyField;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionPlane};
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common;
use common::probe::{cancelled_parked_at, reports};

const WORK_REFUSAL: &str = "liquify strokes exceed the limit of 1 billion field updates; \
    use fewer or shorter strokes or a smaller brush";
const BYTE_LIMIT: &str = "filter params exceeds the supported byte limit";

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}

fn message(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let m = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    m
}

fn taken(out: *mut DrImage, count: usize) -> Option<Vec<u8>> {
    (!out.is_null()).then(|| {
        let v =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), count * 4) }.to_vec();
        unsafe { dr_image_free(out) };
        v
    })
}

/// `dr_image_filter_op` op `liquify`: `(pixels, message)`, `pixels` None for
/// a NULL result, `message` empty when no error was written.
fn run(
    img: *const DrImage,
    params: &str,
    control: *const DrJobControl,
    count: usize,
) -> (Option<Vec<u8>>, String) {
    let op = CString::new("liquify").unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), control, &mut err) };
    (taken(out, count), message(err))
}

/// The filter's result, which must be an image.
fn liquify(w: u32, h: u32, pixels: &[u8], strokes: Value) -> Vec<u8> {
    let img = image(w, h, pixels);
    let (out, message) = run(img, &strokes.to_string(), ptr::null(), (w * h) as usize);
    unsafe { dr_image_free(img) };
    assert!(message.is_empty(), "{message}");
    out.expect("liquify answered the bypass")
}

/// `(scratch bytes, message)` of the byte plan for `op`.
fn plan(img: *const DrImage, op: &str, params: &str) -> (u64, String) {
    let (op, params) = (CString::new(op).unwrap(), CString::new(params).unwrap());
    let mut err: *mut c_char = ptr::null_mut();
    let bytes =
        unsafe { dr_image_filter_op_scratch_bytes(img, op.as_ptr(), params.as_ptr(), &mut err) };
    (bytes, message(err))
}

fn refusal(params: Value) -> String {
    let img = image(8, 8, &[128; 256]);
    let (out, message) = run(img, &params.to_string(), ptr::null(), 64);
    unsafe { dr_image_free(img) };
    assert!(out.is_none(), "{params} was accepted");
    message
}

fn field(w: u32, h: u32) -> *mut LiquifyField {
    let mut err: *mut c_char = ptr::null_mut();
    let field = unsafe { dr_liquify_field_create(w, h, &mut err) };
    assert!(!field.is_null(), "{}", message(err));
    field
}

/// A new field with `strokes` applied after `field`'s own; `field` is kept.
fn applied(field: *const LiquifyField, strokes: &str) -> Result<*mut LiquifyField, String> {
    let text = CString::new(strokes).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let next = unsafe { dr_liquify_field_apply(field, text.as_ptr(), &mut err) };
    if next.is_null() {
        Err(message(err))
    } else {
        Ok(next)
    }
}

/// An `x, y, width, height` rect of an image.
type Rect = (u32, u32, u32, u32);

/// `pixels` (`w` × `h`) through `field`: whole, or `previous` with `rect`
/// re-rendered.
fn rendered(
    field: *const LiquifyField,
    w: u32,
    h: u32,
    pixels: &[u8],
    previous: Option<(&[u8], Rect)>,
) -> Result<Vec<u8>, String> {
    let img = image(w, h, pixels);
    let older = previous.map(|(bytes, _)| image(w, h, bytes));
    let rect = previous.map_or((0, 0, 0, 0), |(_, rect)| rect);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_liquify_field_render(
            field,
            img,
            older.unwrap_or(ptr::null_mut()),
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            &mut err,
        )
    };
    unsafe {
        dr_image_free(img);
        if let Some(older) = older {
            dr_image_free(older);
        }
    }
    taken(out, (w * h) as usize).ok_or_else(|| message(err))
}

/// Structure and mixed alpha: opaque, partial, and transparent pixels with
/// RGB 0 (`filter_motion_lens_tilt.rs`'s plate).
fn plate(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let value = |offset: u32| ((x * 23 + y * 41 + offset + (i % 3) * 17) % 256) as u8;
            if i % 13 == 5 {
                return [0, 0, 0, 0];
            }
            [
                value(0),
                value(80),
                value(160),
                if i % 4 == 1 {
                    128 + (i % 64) as u8
                } else {
                    255
                },
            ]
        })
        .collect()
}

fn pixel(buffer: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]]
}

/// The distance from the centre of pixel `(x, y)` to the point `(cx, cy)`.
fn distance(x: u32, y: u32, cx: f64, cy: f64) -> f64 {
    (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy)
}

fn stroke(tool: &str, size: f64, strength: f64, points: Value) -> Value {
    json!({"tool": tool, "size": size, "strength": strength, "density": 1, "points": points})
}

/// A full-strength push of a 40 px brush from (40, 48) by `(dx, 0)`.
fn push(dx: f64) -> Value {
    json!({"strokes": [stroke("forward_warp", 40.0, 1.0, json!([[40, 48], [40.0 + dx, 48]]))]})
}

#[test]
fn a_new_field_and_a_strength_zero_stroke_change_nothing() {
    let (w, h) = (48_u32, 40_u32);
    let src = plate(w, h);
    let identity = field(w, h);
    assert_eq!(rendered(identity, w, h, &src, None).unwrap(), src);
    // By rect, over a stale preview: the rect comes back as the source.
    let stale = vec![7_u8; src.len()];
    let patched = rendered(identity, w, h, &src, Some((&stale, (5, 6, 20, 10)))).unwrap();
    for y in 0..h {
        for x in 0..w {
            let inside = (5..25).contains(&x) && (6..16).contains(&y);
            let expected = if inside { pixel(&src, w, x, y) } else { [7; 4] };
            assert_eq!(pixel(&patched, w, x, y), expected, "{x},{y}");
        }
    }
    unsafe { dr_liquify_field_free(identity) };

    // Strength 0 moves nothing: the filter answers the bypass, but its plan
    // cannot know that before the strokes run.
    let strokes =
        json!({"strokes": [stroke("forward_warp", 30.0, 0.0, json!([[10, 10], [30, 20]]))]})
            .to_string();
    let img = image(w, h, &src);
    let (bytes, error) = plan(img, "liquify", &strokes);
    assert!(error.is_empty() && bytes > 0, "{bytes} {error}");
    let (out, error) = run(img, &strokes, ptr::null(), (w * h) as usize);
    assert!(out.is_none() && error.is_empty(), "{error}");
    unsafe { dr_image_free(img) };
}

#[test]
fn a_full_strength_push_translates_the_core_exactly_and_leaves_the_rest_alone() {
    let (w, h) = (96_u32, 96_u32);
    let mut src = plate(w, h);
    // Hidden colour under alpha 0, far from the brush: an unmoved pixel keeps it.
    for (x, y) in [(2_u32, 3_u32), (90, 80), (10, 90)] {
        let i = ((y * w + x) * 4) as usize;
        src[i..i + 4].copy_from_slice(&[9, 8, 7, 0]);
    }
    let out = liquify(w, h, &src, push(5.0));
    let (mut moved, mut kept) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let d = distance(x, y, 45.0, 48.0);
            if d <= 18.0 {
                assert_eq!(pixel(&out, w, x, y), pixel(&src, w, x - 5, y), "{x},{y}");
                moved += 1;
            } else if d >= 21.0 {
                assert_eq!(pixel(&out, w, x, y), pixel(&src, w, x, y), "{x},{y}");
                kept += 1;
            }
        }
    }
    assert!(moved > 900 && kept > 7000, "{moved} {kept}");
    assert_ne!(out, src);
}

#[test]
fn a_plane_over_2048_px_pushes_through_a_coarser_grid_and_its_proxy() {
    // 2100 px on the long side: cell = ⌈2100 / 2048⌉ = 2, so the field is
    // 1050 × 8 nodes at the odd plane coordinates (2i + 1, 2j + 1).
    let (w, h) = (2100_u32, 16_u32);
    let src = plate(w, h);
    // r = 20 and step = max(cell, r / 4) = 5, so the 10 px segment is two
    // dabs, at (1005, 8) and (1010, 8), each moving 5 px at weight 1.
    let strokes = json!({"strokes": [
        stroke("forward_warp", 40.0, 1.0, json!([[1000, 8], [1010, 8]])),
    ]});
    let text = strokes.to_string();

    // The plan says which grid this is: 16n for the plane, 20 bytes for each
    // of 1050 × 8 nodes (one node a pixel would be 20n) and 2 MiB, then the
    // byte filter's 4 MiB and its quarter.
    let n = u64::from(w * h);
    let base = 16 * n + 20 * (1050 * 8) + 6 * 1024 * 1024;
    let img = image(w, h, &src);
    let (bytes, error) = plan(img, "liquify", &text);
    unsafe { dr_image_free(img) };
    assert!(error.is_empty(), "{error}");
    assert_eq!(bytes, base + base.div_ceil(4));

    // A node within 18 px (0.9 r) of a dab's centre takes that dab whole. The
    // second dab reads the field 5 px to the left of each node, halfway
    // between the nodes 4 and 6 px to its left, so a node within 17 px of
    // (1010, 8) has both of those within 18 px of (1005, 8) and holds exactly
    // −10. A pixel reads the four nodes around it, each at most 1.5 px away on
    // either axis (2.13 px in all): within 14.5 px of (1010, 8) all four hold
    // −10 and the pixel is the source pixel 10 px to its left; 22.5 px or more
    // from both dab centres all four are untouched (a dab reaches 20 px).
    let out = liquify(w, h, &src, strokes);
    let (mut moved, mut kept) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let d = distance(x, y, 1010.0, 8.0);
            if d <= 14.5 {
                assert_eq!(pixel(&out, w, x, y), pixel(&src, w, x - 10, y), "{x},{y}");
                moved += 1;
            } else if d >= 22.5 && distance(x, y, 1005.0, 8.0) >= 22.5 {
                assert_eq!(pixel(&out, w, x, y), pixel(&src, w, x, y), "{x},{y}");
                kept += 1;
            }
        }
    }
    assert!(moved > 380 && kept > 32_000, "{moved} {kept}");

    // The workspace's proxy of this plane, at half size: proxy pixel (x, y) has
    // its centre on node (x, y), so it shows that node's displacement halved, a
    // whole 5 px, with no interpolation between nodes.
    let (pw, ph) = (1050_u32, 8_u32);
    let proxy = plate(pw, ph);
    let start = field(w, h);
    let pushed = applied(start, &text).unwrap();
    assert_eq!(rendered(pushed, w, h, &src, None).unwrap(), out);
    let shown = rendered(pushed, pw, ph, &proxy, None).unwrap();
    let (mut moved, mut kept) = (0, 0);
    for y in 0..ph {
        for x in 0..pw {
            // The node's own centre, in plane pixels.
            let from = |cx: f64| (f64::from(2 * x + 1) - cx).hypot(f64::from(2 * y + 1) - 8.0);
            if from(1010.0) <= 17.0 {
                assert_eq!(
                    pixel(&shown, pw, x, y),
                    pixel(&proxy, pw, x - 5, y),
                    "{x},{y}"
                );
                moved += 1;
            } else if from(1010.0) >= 20.0 && from(1005.0) >= 20.0 {
                assert_eq!(pixel(&shown, pw, x, y), pixel(&proxy, pw, x, y), "{x},{y}");
                kept += 1;
            }
        }
    }
    assert!(moved > 100 && kept > 8_000, "{moved} {kept}");
    unsafe {
        dr_liquify_field_free(start);
        dr_liquify_field_free(pushed);
    }
}

/// Catmull-Rom at distance `t` from a tap.
fn catmull_rom(t: f64) -> f64 {
    let x = t.abs();
    if x < 1.0 {
        1.5 * x * x * x - 2.5 * x * x + 1.0
    } else if x < 2.0 {
        -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
    } else {
        0.0
    }
}

/// The straight 8-bit levels of `src` sampled at the continuous point
/// `(qx, qy)` (pixel centres at half-integers): 4×4 Catmull-Rom taps over
/// premultiplied samples, taps outside the image dropped and the rest
/// renormalized, colour clamped.
fn bicubic(src: &[u8], w: u32, h: u32, qx: f64, qy: f64) -> [f64; 4] {
    let (sx, sy) = (qx - 0.5, qy - 0.5);
    let (x0, y0) = (sx.floor() as i64, sy.floor() as i64);
    let mut acc = [0.0_f64; 4];
    let mut total = 0.0;
    for ty in -1..=2_i64 {
        for tx in -1..=2_i64 {
            let (px, py) = (x0 + tx, y0 + ty);
            if px < 0 || py < 0 || px >= i64::from(w) || py >= i64::from(h) {
                continue;
            }
            let weight = catmull_rom(sx - px as f64) * catmull_rom(sy - py as f64);
            let p = pixel(src, w, px as u32, py as u32).map(|v| f64::from(v) / 255.0);
            for c in 0..3 {
                acc[c] += weight * p[c] * p[3];
            }
            acc[3] += weight * p[3];
            total += weight;
        }
    }
    let alpha = acc[3] / total;
    if alpha < 0.5 / 65535.0 {
        return [0.0; 4];
    }
    [
        (acc[0] / acc[3]).clamp(0.0, 1.0) * 255.0,
        (acc[1] / acc[3]).clamp(0.0, 1.0) * 255.0,
        (acc[2] / acc[3]).clamp(0.0, 1.0) * 255.0,
        alpha.min(1.0) * 255.0,
    ]
}

#[test]
fn a_fractional_push_is_the_catmull_rom_resample() {
    let (w, h) = (96_u32, 96_u32);
    let src = plate(w, h);
    let out = liquify(w, h, &src, push(2.5));
    let mut checked = 0;
    for y in 0..h {
        for x in 0..w {
            if distance(x, y, 42.5, 48.0) > 18.0 {
                continue;
            }
            let expected = bicubic(&src, w, h, f64::from(x) + 0.5 - 2.5, f64::from(y) + 0.5);
            let got = pixel(&out, w, x, y);
            for c in 0..4 {
                // Colour under an alpha that rounds to nothing is not compared.
                if c < 3 && expected[3] < 0.5 {
                    continue;
                }
                assert!(
                    (f64::from(got[c]) - expected[c]).abs() <= 1.0,
                    "{x},{y} component {c}: {} vs {}",
                    got[c],
                    expected[c]
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 900, "{checked}");
}

#[test]
fn reconstruct_at_full_strength_restores_the_source() {
    let (w, h) = (96_u32, 96_u32);
    let src = plate(w, h);
    let warp = stroke("pucker", 60.0, 1.0, json!([[40, 44], [52, 50], [52, 50]]));
    let undo = stroke("reconstruct", 1000.0, 1.0, json!([[48, 48]]));
    assert_ne!(liquify(w, h, &src, json!({"strokes": [warp.clone()]})), src);
    let both = json!({"strokes": [warp.clone(), undo.clone()]}).to_string();
    let img = image(w, h, &src);
    let (out, error) = run(img, &both, ptr::null(), (w * h) as usize);
    unsafe { dr_image_free(img) };
    assert!(out.is_none() && error.is_empty(), "{error}");

    let start = field(w, h);
    let warped = applied(start, &json!({"strokes": [warp]}).to_string()).unwrap();
    let restored = applied(warped, &json!({"strokes": [undo]}).to_string()).unwrap();
    assert_ne!(rendered(warped, w, h, &src, None).unwrap(), src);
    assert_eq!(rendered(restored, w, h, &src, None).unwrap(), src);
    unsafe {
        dr_liquify_field_free(start);
        dr_liquify_field_free(warped);
        dr_liquify_field_free(restored);
    }
}

#[test]
fn a_frozen_disc_keeps_its_pixels_until_it_is_thawed() {
    let (w, h) = (96_u32, 96_u32);
    let src = plate(w, h);
    let disc = |tool: &str| stroke(tool, 60.0, 1.0, json!([[30, 48]]));
    let sweep = stroke("forward_warp", 200.0, 1.0, json!([[20, 48], [76, 48]]));
    let frozen = liquify(
        w,
        h,
        &src,
        json!({"strokes": [disc("freeze"), sweep.clone()]}),
    );
    let thawed = liquify(
        w,
        h,
        &src,
        json!({"strokes": [disc("freeze"), disc("thaw"), sweep]}),
    );
    let (mut held, mut elsewhere, mut released) = (0, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let same = pixel(&frozen, w, x, y) == pixel(&src, w, x, y);
            if distance(x, y, 30.0, 48.0) <= 26.0 {
                assert!(same, "frozen pixel {x},{y} moved");
                held += 1;
                released += usize::from(pixel(&thawed, w, x, y) != pixel(&src, w, x, y));
            } else {
                elsewhere += usize::from(!same);
            }
        }
    }
    assert!(held > 2000, "{held}");
    assert!(elsewhere > 1000, "{elsewhere}");
    assert!(released > 1000, "{released}");
}

#[test]
fn the_freeze_export_is_the_painted_coverage() {
    let start = field(96, 96);
    let text = json!({"strokes": [stroke("freeze", 60.0, 1.0, json!([[30, 48]]))]}).to_string();
    let frozen = applied(start, &text).unwrap();
    let mut mask = vec![9_u8; 96 * 96];
    assert!(unsafe { dr_liquify_field_freeze(frozen, 96, 96, mask.as_mut_ptr(), mask.len()) });
    for y in 0..96 {
        for x in 0..96 {
            let (d, value) = (distance(x, y, 30.0, 48.0), mask[(y * 96 + x) as usize]);
            if d <= 26.0 {
                assert_eq!(value, 255, "{x},{y}");
            } else if d >= 30.0 {
                assert_eq!(value, 0, "{x},{y}");
            }
        }
    }
    // At half size each sample is the mean of the four nodes around it.
    let mut half = vec![9_u8; 48 * 48];
    assert!(unsafe { dr_liquify_field_freeze(frozen, 48, 48, half.as_mut_ptr(), half.len()) });
    assert_eq!(half[24 * 48 + 15], 255);
    assert_eq!(half[24 * 48 + 40], 0);
    // A buffer of the wrong length is refused and left alone.
    let mut short = vec![9_u8; 10];
    assert!(!unsafe { dr_liquify_field_freeze(frozen, 96, 96, short.as_mut_ptr(), short.len()) });
    assert_eq!(short, [9; 10]);
    unsafe {
        dr_liquify_field_free(start);
        dr_liquify_field_free(frozen);
    }
}

/// The red-weighted centroid of an image.
fn centroid(out: &[u8], w: u32, h: u32) -> (f64, f64) {
    let (mut sx, mut sy, mut total) = (0.0, 0.0, 0.0);
    for y in 0..h {
        for x in 0..w {
            let v = f64::from(pixel(out, w, x, y)[0]);
            sx += v * (f64::from(x) + 0.5);
            sy += v * (f64::from(y) + 0.5);
            total += v;
        }
    }
    (sx / total, sy / total)
}

#[test]
fn each_tool_moves_a_blob_the_way_its_name_says() {
    // A bright 5×5 blob whose centre is 12 px east of the dab centre.
    let (w, h) = (64_u32, 64_u32);
    let src: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let v = if (42..47).contains(&x) && (30..35).contains(&y) {
                255
            } else {
                0
            };
            [v, v, v, 255]
        })
        .collect();
    let before = centroid(&src, w, h);
    assert_eq!(before, (44.5, 32.5));
    let after = |tool: &str| {
        let dabs = json!([[32.5, 32.5], [32.5, 32.5], [32.5, 32.5]]);
        centroid(
            &liquify(
                w,
                h,
                &src,
                json!({"strokes": [stroke(tool, 60.0, 1.0, dabs)]}),
            ),
            w,
            h,
        )
    };
    let pucker = after("pucker");
    assert!(pucker.0 < before.0 - 1.0, "pucker {pucker:?}");
    assert!((pucker.1 - before.1).abs() < 0.25, "pucker {pucker:?}");
    let bloat = after("bloat");
    assert!(bloat.0 > before.0 + 1.0, "bloat {bloat:?}");
    assert!((bloat.1 - before.1).abs() < 0.25, "bloat {bloat:?}");
    let clockwise = after("twirl_clockwise");
    assert!(clockwise.1 > before.1 + 1.0, "clockwise {clockwise:?}");
    let counter = after("twirl_counterclockwise");
    assert!(counter.1 < before.1 - 1.0, "counterclockwise {counter:?}");
}

/// Forty points of a curve across a 96 px plate.
fn curve() -> Vec<[f64; 2]> {
    (0..40)
        .map(|k| {
            let t = f64::from(k);
            [20.0 + t * 1.4, 48.0 + 18.0 * (t * 0.21).sin()]
        })
        .collect()
}

#[test]
fn a_stroke_applied_whole_equals_its_segments_and_the_filter_equals_the_field() {
    let (w, h) = (96_u32, 96_u32);
    let src = plate(w, h);
    let points = curve();
    let one = |points: &[[f64; 2]], continued: bool| {
        json!({"strokes": [{"tool": "forward_warp", "size": 30, "strength": 0.8,
            "density": 0.5, "points": points, "continued": continued}]})
        .to_string()
    };
    let start = field(w, h);
    let whole = applied(start, &one(&points, false)).unwrap();
    let whole_bytes = rendered(whole, w, h, &src, None).unwrap();
    assert_ne!(whole_bytes, src);

    // Point 0, then each segment as a continued two-point stroke. The field
    // after the first 20 segments is kept as the older preview.
    let mut current = applied(start, &one(&points[..1], false)).unwrap();
    let mut older = Vec::new();
    for k in 1..points.len() {
        let next = applied(current, &one(&points[k - 1..=k], true)).unwrap();
        unsafe { dr_liquify_field_free(current) };
        current = next;
        if k == 20 {
            older = rendered(current, w, h, &src, None).unwrap();
        }
    }
    assert_eq!(rendered(current, w, h, &src, None).unwrap(), whole_bytes);

    // The one-call filter builds the same field.
    let text = one(&points, false);
    let filtered = liquify(w, h, &src, serde_json::from_str(&text).unwrap());
    assert_eq!(filtered, whole_bytes);

    // A rect render over the older preview: the later segments' bounding box
    // grown by the brush radius (15) and two nodes covers every node they
    // touched, so the patched preview is the whole render.
    assert_ne!(older, whole_bytes);
    let later = &points[20..];
    let low = |axis: usize| later.iter().map(|p| p[axis]).fold(f64::MAX, f64::min) - 17.0;
    let high = |axis: usize| later.iter().map(|p| p[axis]).fold(f64::MIN, f64::max) + 17.0;
    let (x0, y0) = (
        low(0).floor().max(0.0) as u32,
        low(1).floor().max(0.0) as u32,
    );
    let (x1, y1) = (
        (high(0).ceil() as u32).min(w),
        (high(1).ceil() as u32).min(h),
    );
    let patched = rendered(
        current,
        w,
        h,
        &src,
        Some((&older, (x0, y0, x1 - x0, y1 - y0))),
    )
    .unwrap();
    assert_eq!(patched, whole_bytes);
    unsafe {
        dr_liquify_field_free(start);
        dr_liquify_field_free(whole);
        dr_liquify_field_free(current);
    }
}

#[test]
fn a_render_rect_must_lie_inside_the_image_and_previous_must_match_it() {
    let (w, h) = (32_u32, 24_u32);
    let src = plate(w, h);
    let identity = field(w, h);
    for rect in [(0, 0, 0, 5), (0, 0, 5, 0), (30, 0, 3, 4), (0, 22, 4, 3)] {
        let error = rendered(identity, w, h, &src, Some((&src, rect))).unwrap_err();
        assert_eq!(
            error, "liquify render rect must be a non-empty rectangle inside the image",
            "{rect:?}"
        );
    }
    let img = image(w, h, &src);
    let other = image(8, 8, &[0; 256]);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_liquify_field_render(identity, img, other, 0, 0, 4, 4, &mut err) };
    assert!(out.is_null());
    assert_eq!(
        message(err),
        "liquify previous image must match the image size"
    );
    unsafe {
        dr_image_free(img);
        dr_image_free(other);
        dr_liquify_field_free(identity);
    }
}

#[test]
fn a_proxy_of_half_the_plane_shows_half_the_push() {
    // The field belongs to a 256 px plane; the picture is its 128 px proxy.
    let src = plate(128, 128);
    let start = field(256, 256);
    let text = json!({"strokes": [stroke("forward_warp", 80.0, 1.0,
        json!([[120, 128], [128, 128]]))]})
    .to_string();
    let pushed = applied(start, &text).unwrap();
    let out = rendered(pushed, 128, 128, &src, None).unwrap();
    let mut moved = 0;
    for y in 0..128 {
        for x in 0..128 {
            // 34 plane px from the dab centre is inside the 36 px full core
            // with the node on either side of the proxy pixel's centre.
            if distance(x, y, 64.0, 64.0) * 2.0 <= 34.0 {
                assert_eq!(
                    pixel(&out, 128, x, y),
                    pixel(&src, 128, x - 4, y),
                    "{x},{y}"
                );
                moved += 1;
            }
        }
    }
    assert!(moved > 800, "{moved}");
    unsafe {
        dr_liquify_field_free(start);
        dr_liquify_field_free(pushed);
    }
}

fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn edited16(d: &PrecisionDocument, strokes: &Value, coverage: &[u16]) -> Vec<[u16; 4]> {
    let edit =
        SelectedEdit::parse(&json!({"layer": 0, "op": "liquify", "params": strokes}).to_string())
            .unwrap();
    d.selected_edit(&edit, coverage, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .expect("no change at 16 bits")
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

#[test]
fn sixteen_bits_agree_with_the_bytes_and_coverage_confines_the_result() {
    let (w, h) = (96_u32, 96_u32);
    let src = plate(w, h);
    let wide: Vec<[u16; 4]> = src
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect();
    let d = doc16(w, h, wide.clone());
    let inside = |x: u32, y: u32| (20..70).contains(&x) && (25..75).contains(&y);
    let coverage: Vec<u16> = (0..w * h)
        .map(|i| if inside(i % w, i / w) { 65535 } else { 0 })
        .collect();
    let strokes = json!({"strokes": [
        stroke("forward_warp", 50.0, 0.7, json!([[30, 40], [44, 52], [60, 50]])),
        stroke("twirl_clockwise", 40.0, 1.0, json!([[50, 50], [50, 50]])),
        stroke("bloat", 36.0, 0.9, json!([[40, 60]])),
    ]});
    let out16 = edited16(&d, &strokes, &coverage);
    let out8 = liquify(w, h, &src, strokes);
    let mut changed = 0;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if !inside(x, y) {
                assert_eq!(out16[i], wide[i], "outside {x},{y}");
                continue;
            }
            changed += usize::from(out16[i] != wide[i]);
            for c in 0..4 {
                if c < 3 && out16[i][3] == 0 {
                    continue;
                }
                let got = f64::from(out16[i][c]) / 257.0;
                assert!(
                    (got - f64::from(out8[i * 4 + c])).abs() <= 1.0,
                    "{x},{y} {c}: {got} vs {}",
                    out8[i * 4 + c]
                );
            }
        }
    }
    assert!(changed > 500, "{changed}");
}

#[test]
fn a_sixteen_bit_push_moves_native_codes_exactly() {
    // Codes that no 8-bit level reaches (not multiples of 257), with opaque
    // and partial alpha.
    let (w, h) = (96_u32, 96_u32);
    let wide: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let code = |k: u32| ((x * 523 + y * 1201 + k * 9973 + 41) % 65536) as u16;
            let alpha = if i % 5 == 2 {
                20_001 + (i % 40_000) as u16
            } else {
                65535
            };
            [code(0), code(1), code(2), alpha]
        })
        .collect();
    assert!(wide.iter().flatten().any(|v| v % 257 != 0));
    let out = edited16(&doc16(w, h, wide.clone()), &push(5.0), &[]);
    let mut moved = 0;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let d = distance(x, y, 45.0, 48.0);
            if d <= 18.0 {
                assert_eq!(out[i], wide[i - 5], "{x},{y}");
                moved += 1;
            } else if d >= 21.0 {
                assert_eq!(out[i], wide[i], "{x},{y}");
            }
        }
    }
    assert!(moved > 900, "{moved}");
}

#[test]
fn a_mask_or_channel_target_refuses_liquify() {
    let (w, h) = (8_u32, 6_u32);
    let samples: Vec<u16> = (0..w * h).map(|i| (1000 + i * 1031) as u16).collect();
    let channel = PrecisionChannel::new(
        "Alpha 1",
        PrecisionPlane::from_u16(w, h, samples, None).unwrap(),
    )
    .unwrap();
    let layer = PrecisionDocumentLayer::raster(
        "Layer",
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![[0, 0, 0, 65535]; (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let d = PrecisionDocument::from_parts_and_channels(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
        vec![channel],
    )
    .unwrap();
    let coverage = vec![65535_u16; (w * h) as usize];
    let request = json!({"op": "scalar_edit", "target": "channel", "index": 0, "selection": true,
        "operation": {"kind": "filter", "edit": {"layer": 0, "op": "liquify",
        "params": {"strokes": [stroke("forward_warp", 4.0, 1.0, json!([[2, 2], [4, 3]]))]}}}});
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let planned = unsafe {
        dr_precision_doc_edit_plan(&d, text.as_ptr(), REFERENCE_WORKING_BYTES, 0, &mut err)
    };
    assert!(planned.is_null());
    assert_eq!(message(err), "liquify edits layer pixels only");
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &d,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(message(err), "liquify edits layer pixels only");
}

#[test]
fn parameters_refuse_by_name() {
    let good = || stroke("pucker", 20.0, 0.5, json!([[3, 3]]));
    let with = |key: &str, value: Value| {
        let mut s = good();
        s[key] = value;
        json!({"strokes": [s]})
    };
    let without = |key: &str| {
        let mut s = good();
        s.as_object_mut().unwrap().remove(key);
        json!({"strokes": [s]})
    };
    let strokes = "liquify needs strokes: an array of 1 to 2048 strokes";
    let tool = "liquify tool must be one of forward_warp, pucker, bloat, twirl_clockwise, \
        twirl_counterclockwise, reconstruct, freeze, thaw";
    let size = "liquify size must be finite in [1,5000]";
    let points = "liquify points must be 1 to 8192 finite [x,y] pairs within 1000000";
    let long: Vec<[u32; 2]> = vec![[1, 1]; 8193];
    for (params, wanted) in [
        (json!({}), strokes),
        (json!({"strokes": []}), strokes),
        (json!({"strokes": 3}), strokes),
        (json!({"strokes": [7]}), strokes),
        (json!({"strokes": vec![good(); 2049]}), strokes),
        (
            json!({"strokes": [good()], "radius": 2}),
            "unexpected liquify parameter radius",
        ),
        (
            with("radius", json!(3)),
            "unexpected liquify stroke parameter radius",
        ),
        (with("tool", json!("smear")), tool),
        (with("tool", json!(1)), tool),
        (without("tool"), tool),
        (with("size", json!(0.5)), size),
        (with("size", json!(5001)), size),
        (with("size", json!(true)), size),
        (with("size", json!("20")), size),
        (without("size"), size),
        (
            with("strength", json!(1.5)),
            "liquify strength must be finite in [0,1]",
        ),
        (
            with("strength", json!(true)),
            "liquify strength must be finite in [0,1]",
        ),
        (
            with("density", json!(-0.1)),
            "liquify density must be finite in [0,1]",
        ),
        (
            with("continued", json!(1)),
            "liquify continued must be true or false",
        ),
        (without("points"), points),
        (with("points", json!([])), points),
        (with("points", json!([[1, 2, 3]])), points),
        (with("points", json!([[1]])), points),
        (with("points", json!([[1, true]])), points),
        (with("points", json!([[1, "2"]])), points),
        (with("points", json!([[1_000_001, 0]])), points),
        (with("points", json!([[0, -1_000_000.5]])), points),
        (with("points", json!(long)), points),
    ] {
        assert_eq!(refusal(params.clone()), wanted, "{params}");
    }
    // Defaults: strength and density may be left out; the limits are inclusive.
    let mut bare = good();
    bare.as_object_mut().unwrap().remove("strength");
    bare.as_object_mut().unwrap().remove("density");
    bare["points"] = json!([[-1_000_000, 1_000_000]]);
    bare["size"] = json!(5000);
    let img = image(8, 8, &[128; 256]);
    let (bytes, error) = plan(img, "liquify", &json!({"strokes": [bare]}).to_string());
    unsafe { dr_image_free(img) };
    assert!(error.is_empty() && bytes > 0, "{error}");
}

#[test]
fn the_point_total_is_capped_at_131072() {
    // 16 strokes of 8,192 points are the cap; one more point passes it.
    let full: Vec<[u32; 2]> = vec![[4, 4]; 8192];
    let mut list: Vec<Value> = (0..16)
        .map(|_| stroke("freeze", 4.0, 1.0, json!(full)))
        .collect();
    let img = image(8, 8, &[128; 256]);
    let (bytes, error) = plan(img, "liquify", &json!({"strokes": list}).to_string());
    assert!(error.is_empty() && bytes > 0, "{error}");
    list.push(stroke("freeze", 4.0, 1.0, json!([[4, 4]])));
    let (bytes, error) = plan(img, "liquify", &json!({"strokes": list}).to_string());
    unsafe { dr_image_free(img) };
    assert_eq!(bytes, 0);
    assert_eq!(error, "liquify strokes hold at most 131072 points in all");
}

#[test]
fn strokes_past_a_billion_field_updates_refuse_everywhere_and_keep_the_field() {
    // A 5000 px brush covers the whole 2048 px field: 4,194,304 nodes a dab,
    // so 240 dwell dabs are 1,006,632,960.
    let (w, h) = (2048_u32, 2048_u32);
    let dwell: Vec<[u32; 2]> = vec![[1024, 1024]; 240];
    let text = json!({"strokes": [stroke("bloat", 5000.0, 1.0, json!(dwell))]}).to_string();
    let img = image(w, h, &vec![200_u8; (w * h * 4) as usize]);
    let (bytes, error) = plan(img, "liquify", &text);
    assert_eq!((bytes, error.as_str()), (0, WORK_REFUSAL));
    let (out, error) = run(img, &text, ptr::null(), (w * h) as usize);
    assert!(out.is_none());
    assert_eq!(error, WORK_REFUSAL);
    // Controlled too: it is a parameter refusal, not a work ceiling.
    let control = unsafe { dr_job_control_create() };
    let (out, error) = run(img, &text, control, (w * h) as usize);
    unsafe { dr_job_control_free(control) };
    assert!(out.is_none());
    assert_eq!(error, WORK_REFUSAL);
    unsafe { dr_image_free(img) };

    let start = field(w, h);
    assert_eq!(applied(start, &text).unwrap_err(), WORK_REFUSAL);
    // The refused field is still the identity and still takes a stroke.
    let small = plate(64, 64);
    assert_eq!(rendered(start, 64, 64, &small, None).unwrap(), small);
    let next = applied(
        start,
        &json!({"strokes": [stroke("forward_warp", 400.0, 1.0,
            json!([[1000, 1024], [1100, 1024]]))]})
        .to_string(),
    )
    .unwrap();
    assert_ne!(rendered(next, 64, 64, &small, None).unwrap(), small);
    unsafe {
        dr_liquify_field_free(start);
        dr_liquify_field_free(next);
    }
}

#[test]
fn a_field_built_stroke_by_stroke_refuses_at_the_stroke_the_whole_list_refuses_at() {
    // An 8 px brush has radius 4 and a 1 px step, and its box is 9 × 9 nodes
    // wherever the dab is. A 500,000 px segment far outside a 64 px plane is
    // 500,000 dabs that touch no node and still count: with the dab at point
    // 0 a stroke is 500,001 · 81 = 40,500,081, so 24 strokes (972,001,944)
    // fit under a billion and the 25th (1,012,502,025) does not.
    let (w, h) = (64_u32, 64_u32);
    let one = stroke(
        "pucker",
        8.0,
        1.0,
        json!([[500_000, 500_000], [1_000_000, 500_000]]),
    );
    let list = |count: usize| json!({"strokes": vec![one.clone(); count]}).to_string();
    let img = image(w, h, &plate(w, h));
    let (bytes, error) = plan(img, "liquify", &list(24));
    assert!(error.is_empty() && bytes > 0, "{error}");
    let (out, error) = run(img, &list(24), ptr::null(), (w * h) as usize);
    assert!(out.is_none() && error.is_empty(), "{error}");
    assert_eq!(
        plan(img, "liquify", &list(25)),
        (0, WORK_REFUSAL.to_string())
    );
    unsafe { dr_image_free(img) };

    let mut current = field(w, h);
    for index in 0..24 {
        let next = applied(current, &list(1)).unwrap_or_else(|e| panic!("stroke {index}: {e}"));
        unsafe { dr_liquify_field_free(current) };
        current = next;
    }
    assert_eq!(applied(current, &list(1)).unwrap_err(), WORK_REFUSAL);
    unsafe { dr_liquify_field_free(current) };
}

/// 16 `forward_warp` strokes of 8,192 points each, every coordinate written
/// with 17 significant digits, consecutive points under 1 px apart. The first
/// 12 strokes wander inside a 64 px plate; the last 4 are the same walk 60 px
/// to the left, negative as a layer origin's subtraction leaves them.
fn full_size_request() -> String {
    let mut text = String::from(r#"{"strokes":["#);
    for s in 0..16 {
        if s > 0 {
            text.push(',');
        }
        text.push_str(r#"{"tool":"forward_warp","size":1,"strength":1,"points":["#);
        let shift = if s >= 12 { 60.0 } else { 0.0 };
        for k in 0..8192 {
            if k > 0 {
                text.push(',');
            }
            let t = f64::from(s * 8192 + k);
            let x = 32.0 + 20.0 * (t * 0.011).sin() - shift;
            let y = 32.0 + 20.0 * (t * 0.013).cos();
            text.push_str(&format!("[{x:.16e},{y:.16e}]"));
        }
        text.push_str("]}");
    }
    text.push_str("]}");
    text
}

#[test]
fn the_largest_stroke_list_fits_the_request_cap_and_other_ops_keep_theirs() {
    let (w, h) = (64_u32, 64_u32);
    let src = plate(w, h);
    let img = image(w, h, &src);
    // A 70 KiB params object for another op is still over its 64 KiB cap.
    let padded = format!(r#"{{"radius":5{}}}"#, " ".repeat(70 * 1024));
    assert_eq!(plan(img, "box_blur", &padded), (0, BYTE_LIMIT.to_string()));

    let text = full_size_request();
    assert!(
        text.len() > 4 * 1024 * 1024 && text.len() <= 8 * 1024 * 1024,
        "{} bytes",
        text.len()
    );
    assert!(text.contains("[-"), "no negative coordinate");
    let (bytes, error) = plan(img, "liquify", &text);
    assert!(error.is_empty() && bytes > 0, "{error}");
    let (out, error) = run(img, &text, ptr::null(), (w * h) as usize);
    assert!(error.is_empty(), "{error}");
    assert_ne!(out.expect("an image"), src);
    let start = field(w, h);
    let next = applied(start, &text).expect("the field takes the same text");
    unsafe {
        dr_liquify_field_free(start);
        dr_liquify_field_free(next);
    }

    // One byte past 8 MiB is refused before it is parsed.
    let over = format!("{text}{}", " ".repeat(8 * 1024 * 1024 + 1 - text.len()));
    assert_eq!(over.len(), 8 * 1024 * 1024 + 1);
    assert_eq!(plan(img, "liquify", &over), (0, BYTE_LIMIT.to_string()));
    let (out, error) = run(img, &over, ptr::null(), (w * h) as usize);
    assert!(out.is_none());
    assert_eq!(error, BYTE_LIMIT);
    let start = field(w, h);
    assert_eq!(
        applied(start, &over).unwrap_err(),
        "liquify strokes exceeds the supported byte limit"
    );
    // Exactly 8 MiB is inside the cap.
    let at = &over[..8 * 1024 * 1024];
    let (bytes, error) = plan(img, "liquify", at);
    assert!(error.is_empty() && bytes > 0, "{error}");
    unsafe {
        dr_liquify_field_free(start);
        dr_image_free(img);
    }
}

#[test]
fn the_field_refuses_bad_text_and_bad_planes() {
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_liquify_field_create(0, 10, &mut err) }.is_null());
    assert!(!message(err).is_empty());
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_liquify_field_create(20_000, 20_000, &mut err) }.is_null());
    assert!(!message(err).is_empty());
    let start = field(16, 16);
    assert!(applied(start, "{")
        .unwrap_err()
        .starts_with("invalid liquify strokes"));
    assert_eq!(
        applied(start, "[1]").unwrap_err(),
        "liquify strokes must be a JSON object"
    );
    assert_eq!(
        applied(start, "{}").unwrap_err(),
        "liquify needs strokes: an array of 1 to 2048 strokes"
    );
    assert_eq!(
        applied(
            start,
            r#"{"strokes":[{"tool":"pucker","size":4,"points":[[1,1]]}],"x":1}"#
        )
        .unwrap_err(),
        "unexpected liquify parameter x"
    );
    unsafe { dr_liquify_field_free(start) };
}

#[test]
fn null_arguments_are_tolerated() {
    let (w, h) = (16_u32, 16_u32);
    let src = plate(w, h);
    let img = image(w, h, &src);
    let start = field(w, h);
    let text =
        CString::new(json!({"strokes": [stroke("pucker", 8.0, 1.0, json!([[8, 8]]))]}).to_string())
            .unwrap();
    unsafe {
        // create: a NULL err_out, accepted and refused.
        let made = dr_liquify_field_create(w, h, ptr::null_mut());
        assert!(!made.is_null());
        dr_liquify_field_free(made);
        assert!(dr_liquify_field_create(0, 0, ptr::null_mut()).is_null());

        // apply: a NULL field is NULL with no error; NULL text is an error.
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_liquify_field_apply(ptr::null(), text.as_ptr(), &mut err).is_null());
        assert!(err.is_null());
        assert!(dr_liquify_field_apply(start, ptr::null(), &mut err).is_null());
        assert_eq!(message(err), "liquify strokes is NULL");
        assert!(dr_liquify_field_apply(start, ptr::null(), ptr::null_mut()).is_null());
        let next = dr_liquify_field_apply(start, text.as_ptr(), ptr::null_mut());
        assert!(!next.is_null());

        // render: a NULL field or image is NULL with no error.
        let mut err: *mut c_char = ptr::null_mut();
        let none = dr_liquify_field_render(ptr::null(), img, ptr::null(), 0, 0, 0, 0, &mut err);
        assert!(none.is_null() && err.is_null());
        let none = dr_liquify_field_render(next, ptr::null(), ptr::null(), 0, 0, 0, 0, &mut err);
        assert!(none.is_null() && err.is_null());
        let none = dr_liquify_field_render(next, ptr::null(), img, 0, 0, 4, 4, &mut err);
        assert!(none.is_null() && err.is_null());
        let whole = dr_liquify_field_render(next, img, ptr::null(), 0, 0, 0, 0, ptr::null_mut());
        assert!(!whole.is_null());
        dr_image_free(whole);
        // A refusal with nowhere to write the message.
        let bad = dr_liquify_field_render(next, img, img, 0, 0, 0, 0, ptr::null_mut());
        assert!(bad.is_null());

        // freeze: NULL field, NULL buffer.
        let mut mask = vec![3_u8; (w * h) as usize];
        assert!(!dr_liquify_field_freeze(
            ptr::null(),
            w,
            h,
            mask.as_mut_ptr(),
            mask.len()
        ));
        assert!(mask.iter().all(|v| *v == 3));
        assert!(!dr_liquify_field_freeze(
            next,
            w,
            h,
            ptr::null_mut(),
            mask.len()
        ));
        assert!(!dr_liquify_field_freeze(next, 0, 0, mask.as_mut_ptr(), 0));
        assert!(dr_liquify_field_freeze(
            next,
            w,
            h,
            mask.as_mut_ptr(),
            mask.len()
        ));
        assert!(mask.iter().all(|v| *v == 0));

        // free: NULL is ignored.
        dr_liquify_field_free(ptr::null_mut());
        dr_liquify_field_free(next);
        dr_liquify_field_free(start);

        // The filter op: a NULL image is NULL with no error, and NULL params
        // (no strokes) is the strokes refusal.
        let (out, error) = run(ptr::null(), "{}", ptr::null(), 0);
        assert!(out.is_none() && error.is_empty(), "{error}");
        let op = CString::new("liquify").unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let out = dr_image_filter_op(img, op.as_ptr(), ptr::null(), ptr::null(), &mut err);
        assert!(out.is_null());
        assert_eq!(
            message(err),
            "liquify needs strokes: an array of 1 to 2048 strokes"
        );
        dr_image_free(img);
    }
}

#[test]
fn the_filter_reports_progress_and_cancels_mid_run() {
    let (w, h) = (120_u32, 80_u32);
    let n = (w * h) as usize;
    let img = image(w, h, &plate(w, h)) as usize;
    let params = json!({"strokes": [
        stroke("forward_warp", 50.0, 0.8, json!([[20, 30], [60, 44], [100, 40]])),
        stroke("twirl_clockwise", 40.0, 1.0, json!([[60, 40], [60, 40]])),
    ]})
    .to_string();
    let (plain, error) = run(img as *const DrImage, &params, ptr::null(), n);
    assert!(error.is_empty(), "{error}");
    assert!(plain.is_some(), "a bypass");
    let control = unsafe { dr_job_control_create() };
    let (controlled, error) = run(img as *const DrImage, &params, control, n);
    unsafe { dr_job_control_free(control) };
    assert!(error.is_empty(), "{error}");
    assert!(plain == controlled, "controlled bytes differ");

    let render = |token: usize| {
        let (out, error) = run(
            img as *const DrImage,
            &params,
            token as *const DrJobControl,
            n,
        );
        (out.is_some(), error)
    };
    let log = reports(render);
    assert!(
        log.iter().all(|r| (0.0..=1.0).contains(&r.progress)),
        "a progress outside [0, 1]"
    );
    assert!(
        log.windows(2)
            .all(|pair| pair[0].progress <= pair[1].progress),
        "progress went backwards"
    );
    let last = log.last().expect("a report");
    assert!(
        last.progress == 1.0 && last.stage == 5,
        "the log ends at ({}, stage {})",
        last.progress,
        last.stage
    );
    let inside: Vec<u64> = log
        .iter()
        .filter(|r| r.stage == 6 && r.progress > 0.0 && r.progress < 1.0)
        .map(|r| r.checkpoints)
        .collect();
    assert!(
        inside.len() >= 2,
        "fewer than two stage-6 reports inside (0, 1)"
    );
    // `checkpoint` parks before `report` stores its stage, so the second
    // stage-6 report parks with stage 6 already stored by the first.
    let count = inside[1];
    let parked = cancelled_parked_at(count, &|_, stage| stage == 6, render);
    assert_eq!(parked.count, count);
    unsafe { dr_image_free(img as *mut DrImage) };
}
