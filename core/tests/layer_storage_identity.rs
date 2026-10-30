//! `dr_doc_layer_storage_identity` and its typed twin,
//! `dr_precision_image_storage_identity` on `dr_precision_doc_layer_source`.
//!
//! The oracle is the header contract: equal nonzero values hold the same
//! pixels (a clone, an undo handle, a name/opacity/visibility/offset write);
//! every pixel write answers a new value, even one that leaves every byte
//! equal; 0 for a NULL doc, an out-of-range index or a GROUP. The contract
//! holds only while both rasters are owned, so every comparison below keeps
//! both document handles alive until it has compared.
//!
//! The MASK storage identity (`dr_doc_layer_mask_storage_identity` and
//! `dr_precision_doc_layer_mask_storage_identity`) has the same oracle over
//! the raw mask plane: a rename, a clone and a mask property write keep it,
//! painting or replacing the mask answers a new value, and an entry with no
//! mask, a group, an out-of-range index and NULL answer 0.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::dr_doc_group_layers;
use doctored_core::ffi_identity::{
    dr_doc_layer_mask_storage_identity, dr_doc_layer_storage_identity,
    dr_precision_doc_layer_mask_storage_identity,
};
use doctored_core::ffi_mask::dr_doc_set_mask_properties;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_layer_source, dr_precision_image_free,
    dr_precision_image_storage_identity,
};
use doctored_core::ffi_precision_color::dr_precision_doc_apply_adjustment;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use tempfile::TempDir;

use crate::common;
use common::*;

/// Two raster layers: a red background and a blue top.
fn two_layers(dir: &TempDir) -> *mut DrDocument {
    let doc = doc_from(dir, "base.png", &solid(8, 8, [255, 0, 0, 255]));
    add_layer(
        dir,
        "top.png",
        doc,
        0,
        &solid(8, 8, [0, 0, 255, 255]),
        "Top",
    )
}

fn identity(doc: *const DrDocument, idx: usize) -> u64 {
    unsafe { dr_doc_layer_storage_identity(doc, idx) }
}

/// Runs `op` on `doc`, asserts it produced a document, compares entry `idx`'s
/// identity on both while both are owned, then frees the result.
fn keeps(doc: *const DrDocument, idx: usize, what: &str, op: impl FnOnce() -> *mut DrDocument) {
    let out = op();
    assert!(!out.is_null(), "{what} produced a document");
    let before = identity(doc, idx);
    assert_ne!(before, 0, "{what}: a raster entry answers nonzero");
    assert_eq!(
        identity(out, idx),
        before,
        "{what} keeps the storage identity"
    );
    unsafe { dr_doc_free(out) };
}

#[test]
fn distinct_raster_entries_answer_distinct_nonzero_values() {
    let dir = TempDir::new().unwrap();
    let doc = two_layers(&dir);
    let (a, b) = (identity(doc, 0), identity(doc, 1));
    assert_ne!(a, 0);
    assert_ne!(b, 0);
    assert_ne!(a, b, "two entries own two storages");
    unsafe { dr_doc_free(doc) };
}

#[test]
fn property_writes_and_a_clone_keep_the_value() {
    let dir = TempDir::new().unwrap();
    let doc = two_layers(&dir);
    let name = CString::new("Renamed").unwrap();
    keeps(doc, 1, "a rename", || unsafe {
        dr_doc_with_layer_name(doc, 1, name.as_ptr())
    });
    keeps(doc, 1, "an opacity write", || unsafe {
        dr_doc_with_layer_opacity(doc, 1, 0.5)
    });
    keeps(doc, 1, "a visibility write", || unsafe {
        dr_doc_with_layer_visible(doc, 1, false)
    });
    keeps(doc, 1, "an offset write", || unsafe {
        dr_doc_with_layer_offset(doc, 1, 3, -2)
    });
    keeps(doc, 1, "a clone", || unsafe { dr_doc_clone(doc) });
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_pixel_write_changes_the_value() {
    let dir = TempDir::new().unwrap();
    let doc = two_layers(&dir);
    let image = open_image(&dir, "green.png", &solid(8, 8, [0, 255, 0, 255]));
    let out = unsafe { dr_doc_with_layer_pixels(doc, 1, image) };
    unsafe { dr_image_free(image) };
    assert!(!out.is_null(), "with_layer_pixels produced a document");
    let (before, after) = (identity(doc, 1), identity(out, 1));
    assert_ne!(after, 0);
    assert_ne!(after, before, "new pixels answer a new storage identity");
    assert_eq!(
        identity(out, 0),
        identity(doc, 0),
        "the untouched entry keeps its storage"
    );
    unsafe { dr_doc_free(out) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_group_an_out_of_range_index_and_null_answer_zero() {
    let dir = TempDir::new().unwrap();
    let doc = two_layers(&dir);
    let name = CString::new("G").unwrap();
    let cap = unsafe { dr_doc_layer_count(doc) } + 1;
    let mut group = usize::MAX;
    let mut cleared = vec![0usize; cap];
    let mut cleared_len = 0usize;
    let mut reordered = vec![0usize; cap];
    let mut reordered_len = 0usize;
    let idx = [1usize];
    let grouped = unsafe {
        dr_doc_group_layers(
            doc,
            idx.as_ptr(),
            idx.len(),
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            cap,
        )
    };
    assert!(!grouped.is_null(), "group_layers refused");
    assert_eq!(identity(grouped, group), 0, "a group answers 0");
    let count = unsafe { dr_doc_layer_count(grouped) };
    assert_eq!(
        identity(grouped, count),
        0,
        "an out-of-range index answers 0"
    );
    assert_eq!(identity(ptr::null(), 0), 0, "a NULL doc answers 0");
    unsafe { dr_doc_free(grouped) };
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------------ typed depth --

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) };
    }
}

/// One 3x2 u16 raster record, built directly; every change below goes
/// through the FFI.
fn typed_fixture() -> Owned {
    let profile = IccProfile::srgb();
    let samples: Vec<[u16; 4]> = (0..6u16)
        .map(|i| [1000 + i * 11, 20000 + i * 7, 30000 + i, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(3, 2, samples, Arc::clone(&profile), None).unwrap();
    let document = PrecisionDocument::from_parts(
        3,
        2,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Base", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    Owned(Box::into_raw(Box::new(document)))
}

/// Record `index`'s source storage identity, the Swift wrapper's reading.
fn typed_identity(document: &Owned, index: usize) -> u64 {
    let source = unsafe { dr_precision_doc_layer_source(document.0, index) };
    assert!(!source.is_null(), "a raster record has a source");
    let value = unsafe { dr_precision_image_storage_identity(source) };
    unsafe { dr_precision_image_free(source) };
    value
}

fn typed_edit(document: &Owned, request: &str) -> Owned {
    let c = CString::new(request).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document.0,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    Owned(out)
}

#[test]
fn typed_source_identity_survives_label_and_visibility_but_not_pixels() {
    let document = typed_fixture();
    let before = typed_identity(&document, 0);
    assert_ne!(before, 0);

    let labelled = typed_edit(&document, r#"{"op":"layer_label","layer":0,"label":1}"#);
    assert_eq!(
        typed_identity(&labelled, 0),
        before,
        "a label edit keeps the source storage"
    );
    let hidden = typed_edit(
        &labelled,
        r#"{"op":"layer_visible","layer":0,"visible":false}"#,
    );
    assert_eq!(
        typed_identity(&hidden, 0),
        before,
        "a visibility toggle keeps the source storage"
    );

    let invert = CString::new(
        serde_json::json!({"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}})
            .to_string(),
    )
    .unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let inverted = unsafe {
        dr_precision_doc_apply_adjustment(
            document.0,
            0,
            invert.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!inverted.is_null(), "invert: {}", take_err_string(err));
    let inverted = Owned(inverted);
    let after = typed_identity(&inverted, 0);
    assert_ne!(after, 0);
    assert_ne!(after, before, "a pixel edit answers a new source storage");
}

// ------------------------------------------------------- mask identity --

fn mask_identity(doc: *const DrDocument, idx: usize) -> u64 {
    unsafe { dr_doc_layer_mask_storage_identity(doc, idx) }
}

/// Compares entry `idx`'s mask identity on `doc` and on `op`'s result while
/// both are owned, then frees the result; `same` says which way it must go.
fn mask_after(
    doc: *const DrDocument,
    idx: usize,
    what: &str,
    same: bool,
    op: impl FnOnce() -> *mut DrDocument,
) {
    let out = op();
    assert!(!out.is_null(), "{what} produced a document");
    let (before, after) = (mask_identity(doc, idx), mask_identity(out, idx));
    assert_ne!(before, 0, "{what}: a masked raster answers nonzero");
    assert_ne!(after, 0, "{what}: the result keeps a mask");
    if same {
        assert_eq!(after, before, "{what} keeps the mask identity");
    } else {
        assert_ne!(after, before, "{what} answers a new mask identity");
    }
    unsafe { dr_doc_free(out) };
}

#[test]
fn the_byte_mask_identity_follows_the_mask_plane() {
    let dir = TempDir::new().unwrap();
    let doc = two_layers(&dir);
    assert_eq!(mask_identity(doc, 1), 0, "no mask answers 0");
    let masked = unsafe { dr_doc_adding_layer_mask(doc, 1, MASK_REVEAL_ALL, ptr::null(), 0, 0) };
    assert!(!masked.is_null());
    assert_eq!(mask_identity(masked, 0), 0, "the unmasked entry answers 0");
    assert_ne!(
        mask_identity(masked, 1),
        identity(masked, 1),
        "the mask plane is not the pixel storage"
    );
    let name = CString::new("Renamed").unwrap();
    mask_after(masked, 1, "a rename", true, || unsafe {
        dr_doc_with_layer_name(masked, 1, name.as_ptr())
    });
    mask_after(masked, 1, "a clone", true, || unsafe {
        dr_doc_clone(masked)
    });
    mask_after(masked, 1, "a density/feather write", true, || unsafe {
        dr_doc_set_mask_properties(masked, 1, true, 0.5, 1.5)
    });
    mask_after(masked, 1, "disabling the mask", true, || unsafe {
        dr_doc_with_layer_mask_enabled(masked, 1, false)
    });
    // Black at full alpha over the whole 8x8 canvas: hides every mask pixel.
    let mut overlay = vec![0u8; 8 * 8 * 4];
    for pixel in overlay.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    mask_after(masked, 1, "painting the mask", false, || unsafe {
        dr_doc_painting_layer_mask(masked, 1, overlay.as_ptr(), 8, 8)
    });
    mask_after(masked, 1, "replacing the mask", false, || unsafe {
        dr_doc_adding_layer_mask(masked, 1, MASK_HIDE_ALL, ptr::null(), 0, 0)
    });

    // A masked GROUP answers 0, as does an out-of-range index and NULL.
    let group_name = CString::new("G").unwrap();
    let cap = unsafe { dr_doc_layer_count(masked) } + 1;
    let mut group = usize::MAX;
    let (mut cleared, mut cleared_len) = (vec![0usize; cap], 0usize);
    let (mut reordered, mut reordered_len) = (vec![0usize; cap], 0usize);
    let grouped = unsafe {
        dr_doc_group_layers(
            masked,
            [1usize].as_ptr(),
            1,
            group_name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            cap,
        )
    };
    assert!(!grouped.is_null(), "group_layers refused");
    let group_masked =
        unsafe { dr_doc_adding_layer_mask(grouped, group, MASK_REVEAL_ALL, ptr::null(), 0, 0) };
    assert!(!group_masked.is_null(), "a group takes a mask");
    assert!(unsafe { dr_doc_layer_has_mask(group_masked, group) });
    assert_eq!(
        mask_identity(group_masked, group),
        0,
        "a masked group answers 0"
    );
    let count = unsafe { dr_doc_layer_count(group_masked) };
    assert_eq!(
        mask_identity(group_masked, count),
        0,
        "out of range answers 0"
    );
    assert_eq!(mask_identity(ptr::null(), 0), 0, "a NULL doc answers 0");
    unsafe {
        dr_doc_free(group_masked);
        dr_doc_free(grouped);
        dr_doc_free(masked);
        dr_doc_free(doc);
    }
}

fn typed_mask_identity(document: &Owned, index: usize) -> u64 {
    unsafe { dr_precision_doc_layer_mask_storage_identity(document.0, index) }
}

/// `typed_edit` with an edit's u16 sample payload.
fn typed_edit_with(document: &Owned, request: &str, samples: &[u16]) -> Owned {
    let c = CString::new(request).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document.0,
            c.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    Owned(out)
}

#[test]
fn the_typed_mask_identity_follows_the_mask_plane() {
    let document = typed_fixture();
    assert_eq!(typed_mask_identity(&document, 0), 0, "no mask answers 0");
    let set = r#"{"op":"set_mask","layer":0,"width":3,"height":2}"#;
    let masked = typed_edit_with(&document, set, &[1, 2, 3, 4, 5, 6]);
    let before = typed_mask_identity(&masked, 0);
    assert_ne!(before, 0, "a masked record answers nonzero");
    assert_ne!(
        before,
        typed_identity(&masked, 0),
        "the mask is not the source"
    );

    let labelled = typed_edit(&masked, r#"{"op":"layer_label","layer":0,"label":2}"#);
    assert_eq!(
        typed_mask_identity(&labelled, 0),
        before,
        "a label edit keeps it"
    );
    let renamed = typed_edit(
        &masked,
        r#"{"op":"rename_layer","layer":0,"name":"Renamed"}"#,
    );
    assert_eq!(
        typed_mask_identity(&renamed, 0),
        before,
        "a rename keeps it"
    );
    let properties = typed_edit(
        &masked,
        r#"{"op":"mask_properties","layer":0,"x":0,"y":0,"linked":true,"density":0.5,"feather":0.0,"enabled":true}"#,
    );
    assert_eq!(
        typed_mask_identity(&properties, 0),
        before,
        "a density write keeps it"
    );
    let replaced = typed_edit_with(&masked, set, &[6, 5, 4, 3, 2, 1]);
    let after = typed_mask_identity(&replaced, 0);
    assert_ne!(after, 0);
    assert_ne!(after, before, "replacing the mask answers a new value");
    let removed = typed_edit(&masked, r#"{"op":"remove_mask","layer":0}"#);
    assert_eq!(
        typed_mask_identity(&removed, 0),
        0,
        "a removed mask answers 0"
    );

    let grouped = typed_edit(&masked, r#"{"op":"group_layers","layers":[0],"name":"G"}"#);
    assert_ne!(
        typed_mask_identity(&grouped, 0),
        0,
        "the member keeps its mask"
    );
    let group_masked = typed_edit_with(
        &grouped,
        r#"{"op":"set_mask","layer":1,"width":3,"height":2}"#,
        &[1, 2, 3, 4, 5, 6],
    );
    assert_eq!(
        typed_mask_identity(&group_masked, 1),
        0,
        "a masked group answers 0"
    );
    assert_eq!(
        typed_mask_identity(&grouped, 2),
        0,
        "out of range answers 0"
    );
    assert_eq!(
        unsafe { dr_precision_doc_layer_mask_storage_identity(ptr::null(), 0) },
        0,
        "NULL answers 0"
    );
}
