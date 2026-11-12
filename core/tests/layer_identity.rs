//! Stable layer identity (`doc_identity`), exercised through the public C FFI
//! at byte depth and through the typed API at high depth.
//!
//! The oracle is the RULE, written out here rather than read off the
//! implementation: **a NEW picture gets a new id, the SAME picture carried
//! through an op keeps its own, and a merge keeps the LOWEST merged entry's
//! id.** Every test below names which half of that it is checking, so a future
//! op that starts minting where it should carry — or the reverse — fails here
//! with the reason rather than in a host's per-layer state a week later.
//!
//! Nothing here asserts a particular NUMBER: ids are process-global and every
//! other test file in this binary's run mints from the same counter, so the
//! only claims that can be made are uniqueness, non-zero-ness, and equality or
//! inequality between two ids of the same document.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::*;
use doctored_core::ffi_identity::dr_doc_layer_id;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as TypedLayer};
use doctored_core::precision_image::PrecisionImage;
use tempfile::TempDir;

use crate::common;
use common::*;

// ---------------------------------------------------------------- helpers --

/// Every entry's id, bottom-first.
fn ids(doc: *const DrDocument) -> Vec<u64> {
    (0..unsafe { dr_doc_layer_count(doc) })
        .map(|i| unsafe { dr_doc_layer_id(doc, i) })
        .collect()
}

/// The rule every document must satisfy at every moment: no zero, no repeat.
fn assert_distinct_and_live(doc: *const DrDocument, what: &str) {
    let list = ids(doc);
    assert!(!list.is_empty(), "{what}: a document always has an entry");
    assert!(
        !list.contains(&0),
        "{what}: 0 is never a live id ({list:?})"
    );
    let mut sorted = list.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), list.len(), "{what}: ids repeat ({list:?})");
}

/// One op off an EXISTING handle, keeping it alive — several of the tests
/// below branch the same document three ways to compare what each op did to
/// its ids, where `common::apply` would free the shared handle after the
/// first branch.
fn branch(
    doc: *const DrDocument,
    op: impl FnOnce(*const DrDocument) -> *mut DrDocument,
) -> *mut DrDocument {
    let out = op(doc);
    assert!(!out.is_null(), "document operation failed");
    out
}

/// A three-entry document: "Background", "Middle", "Top".
fn three_layers(dir: &TempDir) -> *mut DrDocument {
    let mut doc = doc_from(dir, "bg.png", &solid(4, 4, RED));
    doc = add_layer(dir, "mid.png", doc, 0, &solid(4, 4, GREEN), "Middle");
    add_layer(dir, "top.png", doc, 1, &solid(4, 4, BLUE), "Top")
}

// ---------------------------------------------------------- the two halves --

#[test]
fn every_entry_has_its_own_non_zero_id() {
    let dir = TempDir::new().unwrap();
    let doc = three_layers(&dir);
    assert_distinct_and_live(doc, "a three-layer document");
    // Out of range answers the same 0 a NULL doc does, which is what makes 0
    // usable as "no layer" on the host side.
    assert_eq!(unsafe { dr_doc_layer_id(doc, 3) }, 0);
    assert_eq!(unsafe { dr_doc_layer_id(ptr::null(), 0) }, 0);
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_rename_an_opacity_and_a_reorder_all_carry_the_id_through() {
    let dir = TempDir::new().unwrap();
    let mut doc = three_layers(&dir);
    let before = ids(doc);

    let cloud = CString::new("Cloud").unwrap();
    doc = apply(doc, |d| unsafe {
        dr_doc_with_layer_name(d, 2, cloud.as_ptr())
    });
    assert_eq!(ids(doc), before, "a rename is the same picture");

    doc = apply(doc, |d| unsafe { dr_doc_with_layer_opacity(d, 1, 0.25) });
    assert_eq!(ids(doc), before, "an opacity change is the same picture");

    // Top-to-bottom: the entries PERMUTE, so the id list permutes with them
    // rather than staying put — which is the whole point of an id over an
    // index.
    doc = apply(doc, |d| unsafe { dr_doc_move_layer_to(d, 2, 0, 0) });
    assert_eq!(
        ids(doc),
        vec![before[2], before[0], before[1]],
        "a reorder moves ids with their entries and mints nothing"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn an_undo_handle_still_carries_the_ids_it_was_made_with() {
    // "Undo" on the host side is a stack of handles, so this is the claim
    // that makes per-layer state survive a ⌘Z: the OLD handle is untouched by
    // anything done to the new one.
    let dir = TempDir::new().unwrap();
    let doc = three_layers(&dir);
    let before = ids(doc);
    let edited = unsafe { dr_doc_with_layer_opacity(doc, 1, 0.5) };
    assert!(!edited.is_null());
    assert_eq!(ids(doc), before, "the undo handle is unchanged");
    assert_eq!(ids(edited), before, "…and the edit carried the ids through");
    unsafe { dr_doc_free(edited) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_duplicated_group_yields_fresh_ids_across_the_whole_copied_subtree() {
    let dir = TempDir::new().unwrap();
    let mut doc = three_layers(&dir);
    // Group the top two entries, so the stack is [Background, Middle, Top,
    // Group] and the group's subtree is three records long.
    let indices = [1usize, 2];
    let name = CString::new("Set").unwrap();
    let mut group = 0usize;
    let mut cleared = [0usize; 8];
    let mut cleared_len = 0usize;
    let mut reordered = [0usize; 8];
    let mut reordered_len = 0usize;
    doc = apply(doc, |d| unsafe {
        dr_doc_group_layers(
            d,
            indices.as_ptr(),
            indices.len(),
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            8,
        )
    });
    let grouped = ids(doc);
    assert_distinct_and_live(doc, "after grouping");
    assert_eq!(grouped.len(), 4);

    // `dr_doc_duplicating_layer` — the single-entry spelling.
    let one = branch(doc, |d| unsafe { dr_doc_duplicating_layer(d, group) });
    assert_distinct_and_live(one, "after duplicating_layer on a group");
    let copied = ids(one);
    assert_eq!(copied.len(), 7, "a three-record subtree was copied");
    for id in &copied[4..] {
        assert!(
            !grouped.contains(id),
            "every copied entry is a new object ({copied:?} vs {grouped:?})"
        );
    }
    assert_eq!(&copied[..4], &grouped[..], "the originals kept their ids");
    unsafe { dr_doc_free(one) };

    // `dr_doc_duplicate_layers` — the set spelling, which loops the same
    // primitive and must therefore behave identically.
    let set = [group];
    let many = branch(doc, |d| unsafe {
        dr_doc_duplicate_layers(d, set.as_ptr(), set.len())
    });
    assert_distinct_and_live(many, "after duplicate_layers on a group");
    let copied = ids(many);
    assert_eq!(copied.len(), 7);
    for id in &copied[4..] {
        assert!(!grouped.contains(id), "every copied entry is a new object");
    }
    unsafe { dr_doc_free(many) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn via_copy_and_stamp_visible_mint() {
    let dir = TempDir::new().unwrap();
    let doc = three_layers(&dir);
    let before = ids(doc);

    let name = CString::new("Via").unwrap();
    let via = branch(doc, |d| unsafe {
        dr_doc_layer_via(d, 1, ptr::null(), 4, 4, false, name.as_ptr())
    });
    assert_distinct_and_live(via, "after Via Copy");
    let after = ids(via);
    assert_eq!(&after[..2], &before[..2]);
    assert!(
        !before.contains(&after[2]),
        "Via Copy rasterizes into a NEW entry"
    );
    unsafe { dr_doc_free(via) };

    let name = CString::new("Stamp").unwrap();
    let stamped = branch(doc, |d| unsafe {
        dr_doc_stamp_visible(d, 2, name.as_ptr())
    });
    assert_distinct_and_live(stamped, "after Stamp Visible");
    let after = ids(stamped);
    assert_eq!(&after[..3], &before[..]);
    assert!(!before.contains(&after[3]), "the stamp is a NEW entry");
    unsafe { dr_doc_free(stamped) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_merge_keeps_the_lowest_merged_entrys_id() {
    let dir = TempDir::new().unwrap();
    let doc = three_layers(&dir);
    let before = ids(doc);

    // Merge Layers over the top two: the result takes index 1's slot, name,
    // locks and link, so it takes index 1's identity too.
    let members = [1usize, 2];
    let merged = branch(doc, |d| unsafe {
        dr_doc_merge_layers(d, members.as_ptr(), members.len())
    });
    assert_eq!(
        ids(merged),
        vec![before[0], before[1]],
        "the merged entry is the lowest member with new pixels"
    );
    unsafe { dr_doc_free(merged) };

    // Merge DOWN is the same rule with no exception (12E-2): the merged entry
    // takes the LOWER entry's slot, name, flags — and so its id and its
    // colour LABEL.
    let labelled = branch(doc, |d| unsafe { dr_doc_with_layer_label(d, 1, 4) });
    let down = branch(labelled, |d| unsafe { dr_doc_merging_down(d, 2) });
    assert_eq!(
        ids(down),
        vec![before[0], before[1]],
        "merge down keeps the LOWER entry's identity"
    );
    assert_eq!(
        unsafe { dr_doc_layer_label(down, 1) },
        4,
        "and the LOWER entry's colour label"
    );
    unsafe { dr_doc_free(down) };
    unsafe { dr_doc_free(labelled) };

    // Merge Visible: everything contributes, so the one surviving entry takes
    // the bottom-most contributing LEAF's id — the same entry it takes its
    // name from.
    let visible = branch(doc, |d| unsafe { dr_doc_merge_visible(d) });
    assert_eq!(ids(visible), vec![before[0]]);
    unsafe { dr_doc_free(visible) };

    // Flatten collapses the whole tree into a NEW entry and says so.
    let flat = branch(doc, |d| unsafe { dr_doc_flattening(d) });
    let after = ids(flat);
    assert_eq!(after.len(), 1);
    assert!(!before.contains(&after[0]), "a flatten mints");
    unsafe { dr_doc_free(flat) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_native_round_trip_keeps_every_id_because_they_are_persisted() {
    let dir = TempDir::new().unwrap();
    let doc = three_layers(&dir);
    let before = ids(doc);
    let path = dir.path().join("identity.drdc");
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, c.as_ptr(), std::ptr::null(), &mut err) },
        "save failed: {}",
        take_err_string(err)
    );
    let mut err: *mut c_char = ptr::null_mut();
    let reopened = unsafe { dr_doc_open(c.as_ptr(), std::ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "open failed: {}", take_err_string(err));
    assert_distinct_and_live(reopened, "a reopened document");
    // Phase 12E-1 inverted this claim: the always-written `LIDS` table
    // carries every id, and the reader ADOPTS it rather than minting over it,
    // so a reopen hands back exactly the ids the document was saved with —
    // row for row, in the same bottom-first order.
    let after = ids(reopened);
    assert_eq!(
        after, before,
        "ids are persisted, so a reopen returns them unchanged"
    );
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_psd_import_mints_too() {
    // The same claim for the other reader. `gradient.psd` is the committed
    // one-layer fixture the PSD tests already share.
    let path = std::path::Path::new("tests/fixtures/gradient.psd");
    if !path.exists() {
        return;
    }
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let first = unsafe { dr_doc_open(c.as_ptr(), std::ptr::null(), &mut err) };
    if first.is_null() {
        // A fixture this build's importer refuses is not this test's subject.
        if !err.is_null() {
            unsafe { dr_string_free(err) };
        }
        return;
    }
    let mut err: *mut c_char = ptr::null_mut();
    let second = unsafe { dr_doc_open(c.as_ptr(), std::ptr::null(), &mut err) };
    assert!(!second.is_null(), "open failed: {}", take_err_string(err));
    assert_distinct_and_live(first, "a PSD import");
    let (a, b) = (ids(first), ids(second));
    for id in &b {
        assert!(!a.contains(id), "two imports of one file share no identity");
    }
    unsafe { dr_doc_free(second) };
    unsafe { dr_doc_free(first) };
}

// ------------------------------------------------------------ the twin at 16 --

fn typed_leaf(name: &str) -> TypedLayer {
    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[10001, 20002, 30003, 65535], [1, 2, 3, 65535]],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    TypedLayer::raster(name, image, (0, 0), 1.0, true).unwrap()
}

fn typed_ids(document: &PrecisionDocument) -> Vec<u64> {
    (0..document.layer_count())
        .map(|i| document.layer_id(i).expect("every record has an id"))
        .collect()
}

#[test]
fn the_typed_document_answers_the_same_two_questions() {
    let group = TypedLayer::isolated_normal(
        "Parent",
        vec![typed_leaf("Lower"), typed_leaf("Upper")],
        1.0,
        true,
    )
    .unwrap();
    let document = PrecisionDocument::from_parts(
        2,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![group],
    )
    .unwrap();
    let list = typed_ids(&document);
    assert_eq!(list.len(), 3);
    assert!(!list.contains(&0));
    let mut sorted = list.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 3, "typed ids are distinct too");
    for (index, id) in list.iter().enumerate() {
        assert_eq!(document.layer_index_by_id(*id), Some(index));
    }
    assert_eq!(document.layer_index_by_id(0), None, "0 is never a live id");
    assert_eq!(document.layer_id(3), None);

    // A typed duplicate is a NEW object through its whole subtree, exactly as
    // the byte duplicate is.
    let duplicated = document
        .duplicate_layer(2)
        .expect("duplicate")
        .expect("a duplicate always changes the document");
    let after = typed_ids(&duplicated);
    assert_eq!(after.len(), 6);
    let fresh: Vec<u64> = after
        .iter()
        .copied()
        .filter(|id| !list.contains(id))
        .collect();
    assert_eq!(fresh.len(), 3, "the copied subtree is entirely new");

    // An edit INSIDE a group leaves the group's own identity alone: the
    // container is the same container with a changed child.
    let renamed = duplicated
        .set_layer_opacity(0, 0.5)
        .expect("opacity")
        .expect("a real change");
    assert_eq!(
        typed_ids(&renamed).last(),
        after.last(),
        "the enclosing group keeps its id through a child's edit"
    );
}
