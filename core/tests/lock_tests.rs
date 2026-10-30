//! The lock EDIT KINDS that are not a plain pixel write — applying a mask
//! (`DR_EDIT_MASK_APPLY`) and merging into an entry (`DR_EDIT_MERGE`) — the
//! whole-document geometry op that is deliberately EXEMPT from the position
//! lock, and the purity latch on the two lock-adjacent setters.
//!
//! `structure_tests` owns the per-lock behaviour and the two sweeps that
//! prove every wrapped export is wired; it is at its size budget, so
//! `doc_lock`'s coverage continues here. Black-box through the FFI.

use std::ffi::CString;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::*;
use tempfile::TempDir;

use crate::common;
use common::*;

fn set_locks(doc: *mut DrDocument, idx: usize, locks: u32) -> *mut DrDocument {
    apply(doc, |d| unsafe { dr_doc_with_layer_locks(d, idx, locks) })
}

/// `set_locks` that KEEPS the input handle, for the loops that lock the same
/// fixture several ways (`apply` frees what it is given).
fn locked_copy(doc: *const DrDocument, idx: usize, locks: u32) -> *mut DrDocument {
    let out = unsafe { dr_doc_with_layer_locks(doc, idx, locks) };
    assert!(!out.is_null(), "locking {idx} with {locks}");
    out
}

fn group(doc: *mut DrDocument, idx: &[usize], name: &str) -> (*mut DrDocument, usize) {
    let cname = CString::new(name).expect("no interior NUL");
    let mut group = usize::MAX;
    let mut len = 0usize;
    let out = unsafe {
        dr_doc_group_layers(
            doc,
            idx.as_ptr(),
            idx.len(),
            cname.as_ptr(),
            &mut group,
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            &mut len,
            0,
        )
    };
    assert!(!out.is_null(), "group_layers({idx:?}) refused");
    unsafe { dr_doc_free(doc) };
    (out, group)
}

/// The alpha of layer `idx`'s first pixel.
fn first_alpha(doc: *const DrDocument, idx: usize) -> u8 {
    layer_pixels(doc, idx)[3]
}

#[test]
fn applying_a_mask_is_refused_on_a_transparency_locked_layer() {
    // Adding, painting, enabling and DELETING a mask leave the layer's alpha
    // alone, which is why only Lock All gates them. APPLYING one multiplies
    // the coverage straight into that alpha — a hide-all mask applied to a
    // transparency-locked layer erased the entire layer, which is exactly
    // the hole an eraser is not allowed to punch.
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "one.png", &solid(4, 4, [10, 20, 30, 255]));
    let locked = set_locks(doc, 0, LOCK_TRANSPARENCY);
    let masked = apply(locked, |d| unsafe {
        dr_doc_adding_layer_mask(d, 0, MASK_HIDE_ALL, std::ptr::null(), 0, 0)
    });
    assert_eq!(
        first_alpha(masked, 0),
        255,
        "a mask does not touch the alpha"
    );

    assert_eq!(
        unsafe { dr_doc_lock_block(masked, 0, EDIT_MASK) },
        0,
        "a plain mask edit is not gated by Transparency"
    );
    assert_eq!(
        unsafe { dr_doc_lock_block(masked, 0, EDIT_MASK_APPLY) },
        LOCK_TRANSPARENCY,
        "…and an APPLY is, so the host can name the lock"
    );
    assert!(
        unsafe { dr_doc_removing_layer_mask(masked, 0, true) }.is_null(),
        "apply is refused"
    );
    // Deleting the mask WITHOUT applying it is untouched.
    let deleted = apply(masked, |d| unsafe {
        dr_doc_removing_layer_mask(d, 0, false)
    });
    assert_eq!(first_alpha(deleted, 0), 255);

    // Unlocked, the same apply goes through and takes the alpha to zero —
    // which is the behaviour the lock exists to prevent.
    let remasked = apply(deleted, |d| unsafe {
        dr_doc_adding_layer_mask(d, 0, MASK_HIDE_ALL, std::ptr::null(), 0, 0)
    });
    let unlocked = set_locks(remasked, 0, 0);
    let applied = apply(unlocked, |d| unsafe {
        dr_doc_removing_layer_mask(d, 0, true)
    });
    assert_eq!(first_alpha(applied, 0), 0);
    unsafe { dr_doc_free(applied) };
}

#[test]
fn a_merge_answers_to_the_destination_entry_s_locks() {
    // Merging is the one op that destroys a layer's contents outright, and
    // it was the one pixel writer with no lock gate at all: Lock Pixels —
    // and even Lock All — let it replace the lower layer's picture, while
    // the agent's refusal text promised the opposite.
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "low.png", &solid(4, 4, [10, 10, 10, 255]));
    doc = add_layer(
        &dir,
        "up.png",
        doc,
        0,
        &solid(4, 4, [200, 200, 200, 255]),
        "Up",
    );
    let pair = [0usize, 1];

    for lock in [LOCK_PIXELS, LOCK_TRANSPARENCY, LOCK_ALL] {
        let locked = locked_copy(doc, 0, lock);
        assert_eq!(
            unsafe { dr_doc_lock_block(locked, 0, EDIT_MERGE) } & lock,
            lock & (LOCK_PIXELS | LOCK_TRANSPARENCY),
            "the query names the bits that stopped it ({lock})"
        );
        assert!(
            unsafe { dr_doc_merging_down(locked, 1) }.is_null(),
            "merge down onto a locked layer ({lock})"
        );
        assert!(
            unsafe { dr_doc_merge_layers(locked, pair.as_ptr(), 2) }.is_null(),
            "merge layers into a locked lowest member ({lock})"
        );
        unsafe { dr_doc_free(locked) };
    }

    // A POSITION lock is not a merge lock, and the UPPER operand is only
    // removed, which no lock blocks.
    let positioned = locked_copy(doc, 0, LOCK_POSITION);
    let merged = apply(positioned, |d| unsafe { dr_doc_merging_down(d, 1) });
    assert_eq!(layer_pixels(merged, 0)[0], 200, "the merge went through");
    unsafe { dr_doc_free(merged) };

    let upper_locked = locked_copy(doc, 1, LOCK_ALL);
    let merged = apply(upper_locked, |d| unsafe { dr_doc_merging_down(d, 1) });
    assert_eq!(unsafe { dr_doc_layer_count(merged) }, 1);
    unsafe { dr_doc_free(merged) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn setting_the_locks_or_the_open_flag_a_layer_already_has_is_a_no_op() {
    // The core's purity rule, and it is load-bearing rather than tidy: the
    // host registers an undo step and dirties the document for every non-nil
    // answer, so an idempotent `set_layer_lock` used to put a phantom step
    // between the user and their last real edit.
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "one.png", &solid(4, 4, [10, 20, 30, 255]));
    assert!(
        unsafe { dr_doc_with_layer_locks(doc, 0, 0) }.is_null(),
        "an unlocked layer is already unlocked"
    );
    doc = set_locks(doc, 0, LOCK_PIXELS);
    assert!(unsafe { dr_doc_with_layer_locks(doc, 0, LOCK_PIXELS) }.is_null());
    assert!(
        unsafe { dr_doc_with_layer_locks(doc, 0, LOCK_PIXELS | 0xF000) }.is_null(),
        "the reserved bits are masked off BEFORE the comparison"
    );
    assert!(!unsafe { dr_doc_with_layer_locks(doc, 0, LOCK_ALL) }.is_null());

    doc = add_layer(
        &dir,
        "top.png",
        doc,
        0,
        &solid(4, 4, [0, 0, 255, 255]),
        "Top",
    );
    let (doc, g) = group(doc, &[1], "G");
    assert!(unsafe { dr_doc_layer_open(doc, g) }, "a new group is open");
    assert!(
        unsafe { dr_doc_with_layer_open(doc, g, true) }.is_null(),
        "it is open already"
    );
    let closed = apply(doc, |d| unsafe { dr_doc_with_layer_open(d, g, false) });
    assert!(unsafe { dr_doc_with_layer_open(closed, g, false) }.is_null());
    assert!(!unsafe { dr_doc_with_layer_open(closed, g, true) }.is_null());
    unsafe { dr_doc_free(closed) };
}

#[test]
fn a_straighten_ignores_the_position_locks_a_set_transform_refuses() {
    // Straightening re-frames the whole picture, which is why dr_doc_crop,
    // dr_doc_geometry and dr_doc_resize beside it consult no layer lock
    // either. Routing it through the SET transform made one locked layer —
    // the Background — refuse an entire document-wide crop, with nothing but
    // a beep to show for it.
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "bg.png", &solid(8, 8, [10, 20, 30, 255]));
    doc = add_layer(
        &dir,
        "top.png",
        doc,
        0,
        &solid(4, 4, [0, 0, 255, 255]),
        "Top",
    );
    let locked = set_locks(doc, 0, LOCK_POSITION);
    // A quarter turn about the canvas centre: exact, so no resampling loss.
    let affine = [0.0f64, 1.0, -1.0, 0.0, 8.0, 0.0];
    let all = [0usize, 1];
    let (refused, error) = affine_set(locked, &all, affine);
    assert!(
        refused.is_null(),
        "the SET transform is all-or-nothing under a position lock"
    );
    assert!(error.unwrap().contains("position locked"));
    let straightened = apply(locked, |d| {
        let request = serde_json::json!({"op":"straighten","matrix":affine,"filter":"nearest",
            "x":0,"y":0,"width":8,"height":8});
        let (out, error) = byte_geometry_edit(d, &request);
        assert!(error.is_none(), "{error:?}");
        out
    });
    assert_eq!(
        layer_offset(straightened, 1),
        (4, 0),
        "every entry followed the matrix, the locked one included"
    );
    // The plain crop beside it is unlocked in exactly the same way.
    assert!(!unsafe { dr_doc_crop(straightened, 1, 1, 4, 4) }.is_null());
    unsafe { dr_doc_free(straightened) };
}

// ------------------------------------------------ inherited locks --

/// `Outside`, then group `G` holding `Child` and group `Inner`, which holds
/// `Grand`. Returns the document and (child, grand, inner, g).
fn nested(dir: &TempDir) -> (*mut DrDocument, [usize; 4]) {
    let mut doc = doc_from(dir, "outside.png", &solid(4, 4, [10, 20, 30, 255]));
    doc = add_layer(
        dir,
        "child.png",
        doc,
        0,
        &solid(4, 4, [40, 50, 60, 255]),
        "Child",
    );
    doc = add_layer(
        dir,
        "grand.png",
        doc,
        1,
        &solid(4, 4, [70, 80, 90, 255]),
        "Grand",
    );
    let (doc, inner) = group(doc, &[2], "Inner");
    let (doc, g) = group(doc, &[1, inner], "G");
    assert_eq!([inner, g], [3, 4], "postorder: child 1, grand 2, inner 3");
    (doc, [1, 2, 3, 4])
}

fn paint(doc: *const DrDocument, idx: usize) -> *mut DrDocument {
    let overlay = [255u8, 0, 0, 255].repeat(16);
    unsafe { dr_doc_painting_layer(doc, idx, overlay.as_ptr(), 4, 4, COMPOSITE_OVER, 1.0) }
}

fn filter(doc: *const DrDocument, idx: usize) -> *mut DrDocument {
    let before = layer_pixels(doc, idx);
    let inverted: Vec<u8> = before
        .chunks_exact(4)
        .flat_map(|p| [255 - p[0], 255 - p[1], 255 - p[2], p[3]])
        .collect();
    let result = unsafe { dr_image_from_rgba(inverted.as_ptr(), 4, 4) };
    assert!(!result.is_null());
    let out = unsafe {
        doctored_core::ffi_scope::dr_doc_apply_layer_pixels(
            doc,
            idx,
            result,
            std::ptr::null(),
            4,
            4,
        )
    };
    unsafe { dr_image_free(result) };
    out
}

fn owner(doc: *const DrDocument, idx: usize, locks: u32) -> Option<usize> {
    let mut out = usize::MAX;
    unsafe { dr_doc_lock_owner(doc, idx, locks, &mut out) }.then_some(out)
}

fn edited(out: *mut DrDocument) -> bool {
    let ok = !out.is_null();
    if ok {
        unsafe { dr_doc_free(out) };
    }
    ok
}

#[test]
fn a_group_s_locks_cover_its_contents() {
    let dir = TempDir::new().unwrap();
    let (doc, [child, grand, inner, g]) = nested(&dir);

    // Pixels on the group: paint and a filter refuse on a child and a
    // grandchild, and the owner the host names is the group.
    let pixels = locked_copy(doc, g, LOCK_PIXELS);
    for idx in [child, grand] {
        assert_eq!(
            unsafe { dr_doc_lock_block(pixels, idx, EDIT_PIXELS) },
            LOCK_PIXELS
        );
        assert!(!edited(paint(pixels, idx)), "paint on {idx}");
        assert!(!edited(filter(pixels, idx)), "filter on {idx}");
        assert_eq!(owner(pixels, idx, LOCK_PIXELS), Some(g));
    }
    // A sibling outside the group stays editable, and so does the child's
    // mask (a mask freezes only under Lock All).
    assert!(edited(paint(pixels, 0)), "outside the group");
    assert_eq!(owner(pixels, 0, LOCK_PIXELS), None);
    assert_eq!(unsafe { dr_doc_lock_block(pixels, child, EDIT_MASK) }, 0);
    // The nearest holder wins: the inner group's own bit, then the child's.
    let inner_too = locked_copy(pixels, inner, LOCK_PIXELS);
    assert_eq!(owner(inner_too, grand, LOCK_PIXELS), Some(inner));
    let grand_too = locked_copy(inner_too, grand, LOCK_PIXELS);
    assert_eq!(owner(grand_too, grand, LOCK_PIXELS), Some(grand));
    unsafe { dr_doc_free(grand_too) };
    unsafe { dr_doc_free(inner_too) };
    unsafe { dr_doc_free(pixels) };

    // Position on the group: moving a child refuses.
    let position = locked_copy(doc, g, LOCK_POSITION);
    let one = [child];
    assert_eq!(
        unsafe { dr_doc_lock_block(position, child, EDIT_POSITION) },
        LOCK_POSITION
    );
    assert!(!edited(unsafe {
        dr_doc_move_layers(position, one.as_ptr(), 1, 1, 0)
    }));
    assert!(!edited(unsafe {
        dr_doc_with_layer_offset(position, child, 1, 0)
    }));
    assert!(
        edited(paint(position, child)),
        "position does not block paint"
    );
    unsafe { dr_doc_free(position) };

    // Lock All on the group freezes a child's mask; the chain's UNION does
    // not — Lock All is one entry holding all three bits.
    let all = locked_copy(doc, g, LOCK_ALL);
    assert_eq!(
        unsafe { dr_doc_lock_block(all, child, EDIT_MASK) },
        LOCK_ALL
    );
    assert!(!edited(unsafe {
        dr_doc_adding_layer_mask(all, child, MASK_HIDE_ALL, std::ptr::null(), 0, 0)
    }));
    assert_eq!(owner(all, child, LOCK_ALL), Some(g));
    unsafe { dr_doc_free(all) };
    let split = locked_copy(doc, g, LOCK_PIXELS | LOCK_POSITION);
    let union = locked_copy(split, child, LOCK_TRANSPARENCY);
    assert_eq!(unsafe { dr_doc_lock_block(union, child, EDIT_MASK) }, 0);
    assert!(edited(unsafe {
        dr_doc_adding_layer_mask(union, child, MASK_HIDE_ALL, std::ptr::null(), 0, 0)
    }));
    unsafe { dr_doc_free(union) };
    unsafe { dr_doc_free(split) };

    // Transparency on the group freezes a child's alpha: an erase is a no-op.
    let transparency = locked_copy(doc, g, LOCK_TRANSPARENCY);
    let everything = [255u8; 16];
    assert!(!edited(unsafe {
        dr_doc_clear_selection(transparency, child, everything.as_ptr(), 4, 4)
    }));
    assert!(edited(unsafe {
        dr_doc_clear_selection(transparency, 0, everything.as_ptr(), 4, 4)
    }));
    let painted = paint(transparency, grand);
    assert!(!painted.is_null(), "colour may still change");
    assert_eq!(first_alpha(painted, grand), 255);
    unsafe { dr_doc_free(painted) };
    unsafe { dr_doc_free(transparency) };

    // Ungrouped, the child is out from under the lock.
    let pixels = locked_copy(doc, g, LOCK_PIXELS);
    let mut len = 0usize;
    let ungrouped = unsafe { dr_doc_ungroup_layer(pixels, g, std::ptr::null_mut(), &mut len, 0) };
    assert!(!ungrouped.is_null());
    assert_eq!(
        unsafe { dr_doc_lock_block(ungrouped, child, EDIT_PIXELS) },
        0
    );
    assert!(edited(paint(ungrouped, child)));
    unsafe { dr_doc_free(ungrouped) };
    unsafe { dr_doc_free(pixels) };

    // The owner query's edges.
    assert_eq!(owner(doc, child, 0), None, "an empty mask");
    assert_eq!(owner(doc, 99, LOCK_PIXELS), None, "out of range");
    assert!(!unsafe { dr_doc_lock_owner(doc, child, LOCK_ALL, std::ptr::null_mut()) });
    unsafe { dr_doc_free(doc) };
}
