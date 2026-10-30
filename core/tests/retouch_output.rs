//! Retouching onto a NEW layer: the insertion rule, the refusals, and the
//! one property the whole design rests on — the new layer composites to the
//! same picture the in-place repair produces.
//!
//! Black-box through the FFI. The oracle for every composite claim is the
//! IN-PLACE op run on the same fixture with the same overlay, which is the
//! comparison that matters: "somewhere else, same picture" is the feature.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_string_free};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::*;
use doctored_core::ffi_heal::{
    dr_doc_clone_to_new_layer, dr_doc_heal_layer, dr_doc_heal_to_new_layer, dr_doc_retouch_source,
    dr_doc_spot_heal_to_new_layer,
};
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

const CURRENT_AND_BELOW: u32 = 1;
/// `doc_lock::LOCK_PIXELS` — bit 1, not bit 0.
const LOCK_PIXELS: u32 = 2;

/// The document and the result of a `doc -> doc` call that may speak.
fn take(out: *mut DrDocument, err: *mut c_char) -> (Option<*mut DrDocument>, Option<String>) {
    let message = (!err.is_null()).then(|| {
        let text = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        text
    });
    ((!out.is_null()).then_some(out), message)
}

/// The current-and-below plate for `idx`, as bytes.
fn plate(doc: *const DrDocument, idx: usize) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let img =
        unsafe { dr_doc_retouch_source(doc, idx, CURRENT_AND_BELOW, false, ptr::null(), &mut err) };
    assert!(err.is_null() && !img.is_null(), "plate refused");
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    out
}

/// A canvas-frame PREMULTIPLIED overlay: `rgb` at full coverage inside the
/// rect, nothing outside — the shape every retouch overlay has.
fn overlay(w: u32, h: u32, rect: (u32, u32, u32, u32), rgb: [u8; 3]) -> Vec<u8> {
    let (x0, y0, x1, y1) = rect;
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * w + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    out
}

/// A premultiplied overlay whose RGB is the PLATE displaced by `(dx, dy)` —
/// what the Clone Stamp and the Healing Brush actually hand the core, since
/// the host does the alignment before the op ever sees a buffer.
fn displaced_overlay(
    plate: &[u8],
    w: u32,
    h: u32,
    rect: (u32, u32, u32, u32),
    (dx, dy): (i32, i32),
) -> Vec<u8> {
    let (x0, y0, x1, y1) = rect;
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in y0..y1 {
        for x in x0..x1 {
            let sx = x as i32 - dx;
            let sy = y as i32 - dy;
            if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                continue;
            }
            let src = ((sy as u32 * w + sx as u32) * 4) as usize;
            let dst = ((y * w + x) * 4) as usize;
            out[dst..dst + 3].copy_from_slice(&plate[src..src + 3]);
            out[dst + 3] = 255;
        }
    }
    out
}

fn heal_in_place(doc: *const DrDocument, idx: usize, ov: &[u8], w: u32, h: u32) -> *mut DrDocument {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_heal_layer(doc, idx, ov.as_ptr(), w, h, 1.0, ptr::null(), &mut err) };
    assert!(err.is_null(), "in-place heal spoke");
    assert!(!out.is_null(), "in-place heal refused");
    out
}

fn heal_to_layer(
    doc: *const DrDocument,
    idx: usize,
    ov: &[u8],
    w: u32,
    h: u32,
    strength: f32,
    plate: &[u8],
) -> (Option<*mut DrDocument>, Option<String>) {
    let name = CString::new("Retouch").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_heal_to_new_layer(
            doc,
            idx,
            ov.as_ptr(),
            w,
            h,
            strength,
            plate.as_ptr(),
            name.as_ptr(),
            ptr::null(),
            &mut err,
        )
    };
    take(out, err)
}

fn clone_to_layer(
    doc: *const DrDocument,
    idx: usize,
    ov: &[u8],
    w: u32,
    h: u32,
    alpha: f32,
) -> (Option<*mut DrDocument>, Option<String>) {
    let name = CString::new("Retouch").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_clone_to_new_layer(doc, idx, ov.as_ptr(), w, h, alpha, name.as_ptr(), &mut err)
    };
    take(out, err)
}

fn group(doc: *mut DrDocument, idx: &[usize], name: &str) -> (*mut DrDocument, usize) {
    let cname = CString::new(name).unwrap();
    let cap = unsafe { dr_doc_layer_count(doc) } + 1;
    let mut group = usize::MAX;
    let mut cleared = vec![0usize; cap];
    let mut cleared_len = 0usize;
    let mut reordered = vec![0usize; cap];
    let mut reordered_len = 0usize;
    let out = unsafe {
        dr_doc_group_layers(
            doc,
            idx.as_ptr(),
            idx.len(),
            cname.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            cap,
        )
    };
    assert!(!out.is_null());
    unsafe { dr_doc_free(doc) };
    (out, group)
}

/// A photograph with a dark blemish, the fixture every heal here repairs.
///
/// The footprints below deliberately extend PAST the blemish — (4,4)-(12,12)
/// around a blemish at (6,6)-(10,10). A footprint covering only the blemish
/// puts the whole solve contour on dark pixels, so the membrane matches the
/// blemish's own illumination and correctly reproduces it, and the op refuses
/// as an identity. That is the shipped rule, not a fixture convenience: a
/// heal takes its illumination from the ring of pixels AROUND the mark.
fn blemished(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        if (6..10).contains(&x) && (6..10).contains(&y) {
            Rgba([20, 20, 20, 255])
        } else {
            // A gentle ramp, so a membrane solve has real illumination to
            // match rather than a flat field that any implementation gets
            // right by accident.
            let v = (120 + x * 3 + y * 2) as u8;
            Rgba([v, v, v, 255])
        }
    })
}

// ------------------------------------------------------- the composite --

/// THE property the design rests on: healing onto a new layer composites to
/// the same picture as healing in place.
///
/// The oracle is the in-place op on the same fixture with the same overlay.
/// If the deposit's alpha, its domain or the strength's placement were wrong,
/// this is where it shows.
#[test]
fn a_new_layer_heal_composites_to_the_in_place_result() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &blemished(16, 16));
    let plate = plate(doc, 0);
    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));

    let inplace = heal_in_place(doc, 0, &ov, 16, 16);
    let (layered, message) = heal_to_layer(doc, 0, &ov, 16, 16, 1.0, &plate);
    assert!(message.is_none(), "unexpected message: {message:?}");
    let layered = layered.expect("the heal must land on a new layer");

    assert_eq!(
        unsafe { dr_doc_layer_count(layered) },
        2,
        "the repair is on its own layer"
    );
    assert_eq!(
        flat_pixels(layered),
        flat_pixels(inplace),
        "the composite is the in-place picture, byte for byte"
    );
    unsafe { dr_doc_free(inplace) };
    unsafe { dr_doc_free(layered) };
    unsafe { dr_doc_free(doc) };
}

/// …and at a PARTIAL strength, which is where the arithmetic actually bites.
/// A design that put the strength in the deposit's alpha rather than in the
/// solve would apply it twice; one that left the lerp reading the new
/// transparent layer would fade the repair toward BLACK.
#[test]
fn a_partial_strength_new_layer_heal_matches_too() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &blemished(16, 16));
    let plate = plate(doc, 0);
    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));

    let mut err: *mut c_char = ptr::null_mut();
    let inplace =
        unsafe { dr_doc_heal_layer(doc, 0, ov.as_ptr(), 16, 16, 0.5, ptr::null(), &mut err) };
    assert!(err.is_null() && !inplace.is_null());

    let (layered, message) = heal_to_layer(doc, 0, &ov, 16, 16, 0.5, &plate);
    assert!(message.is_none(), "unexpected message: {message:?}");
    let layered = layered.expect("the heal must land");

    let a = flat_pixels(layered);
    let b = flat_pixels(inplace);
    // Exact but for the deposited alpha's own quantization: strength 0.5 is
    // 128/255, which is not 0.5 exactly, so one code of difference inside the
    // patch is arithmetic rather than a defect. Outside it nothing moved at
    // all, which the next assertion checks separately.
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!(
            x.abs_diff(*y) <= 1,
            "byte {i} differs by more than the deposited alpha's own rounding: {x} vs {y}"
        );
    }
    unsafe { dr_doc_free(inplace) };
    unsafe { dr_doc_free(layered) };
    unsafe { dr_doc_free(doc) };
}

/// The original pixels are untouched — the point of the feature. Nothing in
/// the target layer moves, so the repair is undoable by deleting one layer.
#[test]
fn the_target_layers_pixels_are_unchanged() {
    let dir = TempDir::new().unwrap();
    let source = blemished(16, 16);
    let doc = doc_from(&dir, "photo.png", &source);
    let plate = plate(doc, 0);
    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));
    let (layered, _) = heal_to_layer(doc, 0, &ov, 16, 16, 1.0, &plate);
    let layered = layered.expect("the heal must land");

    let raw = unsafe { dr_doc_layer_canvas_image(layered, 0) };
    assert!(!raw.is_null());
    assert_eq!(
        img_pixels(raw),
        source.as_raw().as_slice(),
        "the photograph is exactly as it was"
    );
    unsafe { dr_image_free(raw) };
    unsafe { dr_doc_free(layered) };
    unsafe { dr_doc_free(doc) };
}

/// The deposit is confined to the footprint: outside it the new layer is
/// fully transparent, so the picture there is the original's.
#[test]
fn the_new_layer_is_transparent_outside_the_footprint() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &blemished(16, 16));
    let plate = plate(doc, 0);
    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));
    let (layered, _) = heal_to_layer(doc, 0, &ov, 16, 16, 1.0, &plate);
    let layered = layered.expect("the heal must land");

    let raw = unsafe { dr_doc_layer_canvas_image(layered, 1) };
    assert!(!raw.is_null());
    let repair = img_pixels(raw);
    unsafe { dr_image_free(raw) };

    assert_eq!(pixel(&repair, 16, 0, 0), [0, 0, 0, 0]);
    assert_eq!(pixel(&repair, 16, 15, 15), [0, 0, 0, 0]);
    assert_eq!(
        pixel(&repair, 16, 7, 7)[3],
        255,
        "and opaque where the footprint covered"
    );
    unsafe { dr_doc_free(layered) };
    unsafe { dr_doc_free(doc) };
}

/// A clone onto a new layer is the overlay itself: an OVER composite onto
/// emptiness deposits exactly what was handed in.
#[test]
fn a_new_layer_clone_deposits_the_overlay() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &blemished(16, 16));
    let ov = overlay(16, 16, (2, 2, 6, 6), [10, 200, 30]);
    let (layered, message) = clone_to_layer(doc, 0, &ov, 16, 16, 1.0);
    assert!(message.is_none(), "unexpected message: {message:?}");
    let layered = layered.expect("the clone must land");

    let raw = unsafe { dr_doc_layer_canvas_image(layered, 1) };
    let repair = img_pixels(raw);
    unsafe { dr_image_free(raw) };
    assert_eq!(pixel(&repair, 16, 3, 3), [10, 200, 30, 255]);
    assert_eq!(pixel(&repair, 16, 10, 10), [0, 0, 0, 0]);
    assert_eq!(
        pixel(&flat_pixels(layered), 16, 3, 3),
        [10, 200, 30, 255],
        "and it composites over the photograph"
    );
    unsafe { dr_doc_free(layered) };
    unsafe { dr_doc_free(doc) };
}

// -------------------------------------------------------- the placement --

/// The new layer lands directly above its target, at the target's own depth
/// and INSIDE the same group — not at the top of the stack, where it would be
/// composited past everything between.
#[test]
fn the_new_layer_lands_directly_above_its_target_inside_the_group() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "bottom.png", &blemished(16, 16));
    doc = add_layer(&dir, "skin.png", doc, 0, &blemished(16, 16), "Skin");
    let (doc, group_idx) = group(doc, &[1], "G");
    assert_eq!(group_idx, 2);

    let plate = plate(doc, 1);
    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));
    let (layered, message) = heal_to_layer(doc, 1, &ov, 16, 16, 1.0, &plate);
    assert!(message.is_none(), "unexpected message: {message:?}");
    let layered = layered.expect("the heal must land");

    assert_eq!(unsafe { dr_doc_layer_count(layered) }, 4);
    assert_eq!(
        layer_name(layered, 2),
        "Retouch",
        "the repair sits immediately above Skin"
    );
    assert_eq!(
        unsafe { dr_doc_layer_depth(layered, 2) },
        unsafe { dr_doc_layer_depth(layered, 1) },
        "at the target's own depth, so it is inside the same group"
    );
    assert_eq!(layer_name(layered, 3), "G", "and the group still closes it");
    unsafe { dr_doc_free(layered) };
    unsafe { dr_doc_free(doc) };
}

/// The new layer inherits the target's CLIPPED flag. Without it an unclipped
/// insert above a clipped target would END that target's clip run, re-basing
/// every entry above onto the near-empty repair.
#[test]
fn the_new_layer_inherits_the_targets_clipped_flag() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "base.png", &blemished(16, 16));
    doc = add_layer(&dir, "skin.png", doc, 0, &blemished(16, 16), "Skin");
    doc = apply(doc, |d| unsafe { dr_doc_with_layer_clipped(d, 1, true) });

    let plate = plate(doc, 1);
    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));
    let (layered, _) = heal_to_layer(doc, 1, &ov, 16, 16, 1.0, &plate);
    let layered = layered.expect("the heal must land");
    assert!(
        unsafe { dr_doc_layer_clipped(layered, 2) },
        "a repair above a clipped target is clipped too, or it breaks the run"
    );
    unsafe { dr_doc_free(layered) };
    unsafe { dr_doc_free(doc) };
}

// --------------------------------------------------------- the refusals --

/// A group whose own blending, opacity, mask or style would gate the repair a
/// second time is refused WITH a sentence — the plate already carries that
/// gate, so the layer would be gated twice and the deposit's hard alpha edge
/// would show the difference as an outline in the shape of the stroke.
#[test]
fn a_gating_group_refuses_with_a_reason() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "bottom.png", &blemished(16, 16));
    doc = add_layer(&dir, "skin.png", doc, 0, &blemished(16, 16), "Skin");
    let (mut doc, group_idx) = group(doc, &[1], "G");
    doc = apply(doc, |d| unsafe {
        dr_doc_with_layer_opacity(d, group_idx, 0.5)
    });

    let plate = plate(doc, 1);
    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));
    let (layered, message) = heal_to_layer(doc, 1, &ov, 16, 16, 1.0, &plate);
    assert!(layered.is_none(), "a gating group must refuse");
    let message = message.expect("and must say why");
    assert!(
        message.contains("gated twice") && message.contains('G'),
        "the refusal names the group and the reason: {message}"
    );
    unsafe { dr_doc_free(doc) };
}

/// A pass-through group with no gate of its own is fine: it composites its
/// level onto the backdrop, which is exactly what the plate is.
#[test]
fn a_plain_pass_through_group_is_allowed() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "bottom.png", &blemished(16, 16));
    doc = add_layer(&dir, "skin.png", doc, 0, &blemished(16, 16), "Skin");
    let (doc, _) = group(doc, &[1], "G");

    let plate = plate(doc, 1);
    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));
    let (layered, message) = heal_to_layer(doc, 1, &ov, 16, 16, 1.0, &plate);
    assert!(message.is_none(), "unexpected message: {message:?}");
    assert!(layered.is_some());
    unsafe { dr_doc_free(layered.unwrap()) };
    unsafe { dr_doc_free(doc) };
}

/// A GROUP target has no pixels of its own to retouch, so there is nothing
/// for the repair to sit above.
#[test]
fn a_group_target_refuses_with_a_reason() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "bottom.png", &blemished(16, 16));
    doc = add_layer(&dir, "skin.png", doc, 0, &blemished(16, 16), "Skin");
    let (doc, group_idx) = group(doc, &[1], "G");

    let ov = overlay(16, 16, (2, 2, 6, 6), [10, 200, 30]);
    let (layered, message) = clone_to_layer(doc, group_idx, &ov, 16, 16, 1.0);
    assert!(layered.is_none());
    let message = message.expect("a group must say why");
    assert!(message.contains("group"), "{message}");
    unsafe { dr_doc_free(doc) };
}

/// The PIXEL LOCK belongs to the photograph, not to the brand-new layer: a
/// locked target refuses even though the layer written is unlocked. A silent
/// refusal, like every other lock refusal in the crate.
#[test]
fn a_pixel_locked_target_refuses_silently() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &blemished(16, 16));
    let plate = plate(doc, 0);
    let locked = apply(doc, |d| unsafe {
        dr_doc_with_layer_locks(d, 0, LOCK_PIXELS)
    });

    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));
    let (layered, message) = heal_to_layer(locked, 0, &ov, 16, 16, 1.0, &plate);
    assert!(layered.is_none(), "a pixel-locked layer refuses");
    assert!(message.is_none(), "a lock refusal is silent");

    let (cloned, message) = clone_to_layer(locked, 0, &ov, 16, 16, 1.0);
    assert!(cloned.is_none() && message.is_none());
    unsafe { dr_doc_free(locked) };
}

/// A gesture that deposits nothing mints no layer: an identity result is a
/// refusal, so an empty stroke leaves no blank layer and no undo step.
#[test]
fn a_gesture_that_deposits_nothing_creates_no_layer() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &blemished(16, 16));
    let plate = plate(doc, 0);
    let empty = vec![0u8; 16 * 16 * 4];

    let (layered, message) = heal_to_layer(doc, 0, &empty, 16, 16, 1.0, &plate);
    assert!(layered.is_none() && message.is_none());
    let (cloned, message) = clone_to_layer(doc, 0, &empty, 16, 16, 1.0);
    assert!(cloned.is_none() && message.is_none());
    unsafe { dr_doc_free(doc) };
}

/// NULL and shape refusals, the same rule every overlay-taking export makes
/// before it dereferences a pointer.
#[test]
fn null_and_mismatched_buffers_refuse_cleanly() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &blemished(16, 16));
    let plate = plate(doc, 0);
    let ov = displaced_overlay(&plate, 16, 16, (4, 4, 12, 12), (5, 0));
    let name = CString::new("Retouch").unwrap();

    for (out, label) in [
        (
            unsafe {
                dr_doc_heal_to_new_layer(
                    ptr::null(),
                    0,
                    ov.as_ptr(),
                    16,
                    16,
                    1.0,
                    plate.as_ptr(),
                    name.as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                )
            },
            "null document",
        ),
        (
            unsafe {
                dr_doc_heal_to_new_layer(
                    doc,
                    0,
                    ptr::null(),
                    16,
                    16,
                    1.0,
                    plate.as_ptr(),
                    name.as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                )
            },
            "null overlay",
        ),
        (
            unsafe {
                dr_doc_heal_to_new_layer(
                    doc,
                    0,
                    ov.as_ptr(),
                    16,
                    16,
                    1.0,
                    ptr::null(),
                    name.as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                )
            },
            "null plate",
        ),
        (
            unsafe {
                dr_doc_heal_to_new_layer(
                    doc,
                    0,
                    ov.as_ptr(),
                    8,
                    8,
                    1.0,
                    plate.as_ptr(),
                    name.as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                )
            },
            "wrong canvas size",
        ),
        (
            unsafe {
                dr_doc_clone_to_new_layer(
                    ptr::null(),
                    0,
                    ov.as_ptr(),
                    16,
                    16,
                    1.0,
                    name.as_ptr(),
                    ptr::null_mut(),
                )
            },
            "null document (clone)",
        ),
        (
            unsafe {
                dr_doc_spot_heal_to_new_layer(
                    ptr::null(),
                    0,
                    ov.as_ptr(),
                    16,
                    16,
                    1.0,
                    0,
                    0,
                    0,
                    plate.as_ptr(),
                    name.as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                )
            },
            "null document (spot heal)",
        ),
    ] {
        assert!(out.is_null(), "{label} must refuse");
    }
    // A NULL name is legal: the core supplies its own default.
    let (layered, _) = take(
        unsafe {
            dr_doc_clone_to_new_layer(
                doc,
                0,
                ov.as_ptr(),
                16,
                16,
                1.0,
                ptr::null(),
                ptr::null_mut(),
            )
        },
        ptr::null_mut(),
    );
    let layered = layered.expect("a NULL name is legal");
    assert_eq!(layer_name(layered, 1), "Retouch");
    unsafe { dr_doc_free(layered) };
    unsafe { dr_doc_free(doc) };
}

// -------------------------------------------------------- the spot heal --

/// Spot healing onto a new layer produces the same picture as spot healing in
/// place. The plate is both what the inpaint samples and the illumination it
/// matches, since on an empty layer there is nothing else to read.
#[test]
fn a_new_layer_spot_heal_composites_to_the_in_place_result() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &blemished(24, 24));
    let plate = plate(doc, 0);
    // Pure coverage: the spot heal reads only the overlay's alpha.
    let ov = overlay(24, 24, (4, 4, 12, 12), [0, 0, 0]);

    let (inplace, message) = spot_heal_job(doc, 0, &ov, (24, 24), 1.0, (0, 7, 0), false);
    assert!(message.is_none(), "in-place spot heal spoke");
    let inplace = Box::into_raw(Box::new(inplace.expect("in-place spot heal refused")));

    let name = CString::new("Retouch").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let layered = unsafe {
        dr_doc_spot_heal_to_new_layer(
            doc,
            0,
            ov.as_ptr(),
            24,
            24,
            1.0,
            0,
            7,
            0,
            plate.as_ptr(),
            name.as_ptr(),
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "new-layer spot heal spoke");
    assert!(!layered.is_null(), "new-layer spot heal refused");

    assert_eq!(unsafe { dr_doc_layer_count(layered) }, 2);
    assert_eq!(
        flat_pixels(layered),
        flat_pixels(inplace),
        "same seed, same plate, same picture"
    );
    unsafe { dr_doc_free(inplace) };
    unsafe { dr_doc_free(layered) };
    unsafe { dr_doc_free(doc) };
}
