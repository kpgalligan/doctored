//! WHICH pixels a retouch gesture samples: the three modes, the
//! ignore-adjustments control, and the contribution CUT they are built on.
//!
//! Black-box through `dr_doc_retouch_source`, against oracles computed by
//! hand or by a second FFI path that shares no code with the cut — a hidden
//! document, a removed layer, a single-layer projection. The one thing these
//! tests are really for is stated in `identity`: a cut must change what the
//! plate CONTAINS and nothing whatever about how the rest of the document
//! composites.

use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::*;
use doctored_core::ffi_heal::dr_doc_retouch_source;
use tempfile::TempDir;

use crate::common;
use common::*;

const CURRENT: u32 = 0;
const CURRENT_AND_BELOW: u32 = 1;
const ALL: u32 = 2;
const PASS_THROUGH: i32 = 27;

/// The plate for one mode, asserting it was produced.
fn plate(doc: *const DrDocument, idx: usize, mode: u32, ignore: bool) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_retouch_source(doc, idx, mode, ignore, ptr::null(), &mut err) };
    assert!(err.is_null(), "unexpected message");
    assert!(!img.is_null(), "plate(mode {mode}) refused");
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    out
}

/// The plate, whatever it is: `(pixels, message)`.
fn try_plate(
    doc: *const DrDocument,
    idx: usize,
    mode: u32,
    ignore: bool,
) -> (Option<Vec<u8>>, Option<String>) {
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_retouch_source(doc, idx, mode, ignore, ptr::null(), &mut err) };
    let message = (!err.is_null()).then(|| {
        let text = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { doctored_core::ffi::dr_string_free(err) };
        text
    });
    if img.is_null() {
        return (None, message);
    }
    let out = img_pixels(img);
    unsafe { dr_image_free(img) };
    (Some(out), message)
}

fn set_visible(doc: *mut DrDocument, idx: usize, visible: bool) -> *mut DrDocument {
    apply(doc, |d| unsafe {
        dr_doc_with_layer_visible(d, idx, visible)
    })
}

fn set_clipped(doc: *mut DrDocument, idx: usize, clipped: bool) -> *mut DrDocument {
    apply(doc, |d| unsafe {
        dr_doc_with_layer_clipped(d, idx, clipped)
    })
}

/// `apply_or_keep`, because a new GROUP is already Pass Through: the core
/// answers NULL for a setter that would change nothing (its purity rule), and
/// that is the fixture getting what it asked for, not a failure.
fn set_blend(doc: *mut DrDocument, idx: usize, mode: i32) -> *mut DrDocument {
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_blend_mode(d, idx, mode)
    })
}

/// The document with entry `idx` removed, WITHOUT freeing the original —
/// the oracle for a cut, built by a path that shares no code with it. (The
/// shared `apply` helper frees its input, which a caller that still needs the
/// original must not do.)
fn without_layer(doc: *const DrDocument, idx: usize) -> *mut DrDocument {
    let out = unsafe { dr_doc_removing_layer(doc, idx) };
    assert!(!out.is_null(), "removing_layer({idx}) refused");
    out
}

/// `dr_doc_group_layers`, returning the new document and the group's index.
fn group(doc: *mut DrDocument, idx: &[usize], name: &str) -> (*mut DrDocument, usize) {
    let cname = std::ffi::CString::new(name).unwrap();
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
    assert!(!out.is_null(), "group_layers({idx:?}) refused");
    unsafe { dr_doc_free(doc) };
    (out, group)
}

// ------------------------------------------------------------- the modes --

/// `all` is the document's own projection, byte for byte. It is the historic
/// `sample_all_layers: true`, and the whole compatibility story rests on it
/// being the SAME picture and not merely a similar one.
#[test]
fn all_layers_is_the_documents_own_projection() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "photo.png", &opaque_pattern(24, 16));
    doc = add_layer(
        &dir,
        "over.png",
        doc,
        0,
        &solid(24, 16, [200, 40, 40, 128]),
        "Over",
    );
    assert_eq!(
        plate(doc, 0, ALL, false),
        flat_pixels(doc),
        "the all-layers plate is the projection"
    );
    unsafe { dr_doc_free(doc) };
}

/// `current` is the layer's own pixels placed on a transparent canvas —
/// `dr_doc_layer_canvas_image`, deliberately raw. It is the historic
/// `sample_all_layers: false`.
#[test]
fn current_layer_is_the_layers_own_canvas_image() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "photo.png", &opaque_pattern(24, 16));
    // A small layer at an offset, so "placed on a transparent canvas" is a
    // claim with content: most of the plate must be transparent.
    doc = add_layer(
        &dir,
        "spot.png",
        doc,
        0,
        &solid(6, 4, [10, 220, 30, 255]),
        "Spot",
    );
    doc = apply(doc, |d| unsafe { dr_doc_with_layer_offset(d, 1, 5, 3) });

    let raw = unsafe { dr_doc_layer_canvas_image(doc, 1) };
    assert!(!raw.is_null());
    let oracle = img_pixels(raw);
    unsafe { dr_image_free(raw) };

    let sampled = plate(doc, 1, CURRENT, false);
    assert_eq!(
        sampled, oracle,
        "the current-layer plate is the raw layer image"
    );
    assert_eq!(pixel(&sampled, 24, 5, 3), [10, 220, 30, 255]);
    assert_eq!(
        pixel(&sampled, 24, 0, 0),
        [0, 0, 0, 0],
        "outside the layer's extent the plate is transparent, so a dab there samples nothing"
    );
    unsafe { dr_doc_free(doc) };
}

/// `current_and_below` excludes what is above and keeps what is below — the
/// mode the whole slice exists for. The oracle is the projection of the same
/// document with the layer above REMOVED, which shares no code with the cut.
#[test]
fn current_and_below_excludes_only_what_is_above() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "bottom.png", &solid(8, 8, [20, 20, 20, 255]));
    doc = add_layer(
        &dir,
        "mid.png",
        doc,
        0,
        &solid(8, 8, [90, 90, 90, 200]),
        "Mid",
    );
    doc = add_layer(
        &dir,
        "top.png",
        doc,
        1,
        &solid(8, 8, [240, 10, 10, 255]),
        "Top",
    );

    let sampled = plate(doc, 1, CURRENT_AND_BELOW, false);
    let bare = without_layer(doc, 2);
    assert_eq!(
        sampled,
        flat_pixels(bare),
        "the plate for the middle layer is the picture with the top layer gone"
    );
    assert_ne!(
        sampled,
        flat_pixels(doc),
        "and it is NOT the whole composite, which the top layer covers"
    );
    unsafe { dr_doc_free(bare) };
    unsafe { dr_doc_free(doc) };
}

/// For the TOP layer the two composite modes coincide: there is nothing above
/// it to leave out. Worth pinning because it is the case a user hits first,
/// and a cut that quietly dropped the target itself would pass every other
/// test here.
#[test]
fn current_and_below_on_the_top_layer_is_all_layers() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "bottom.png", &solid(8, 8, [20, 20, 20, 255]));
    doc = add_layer(
        &dir,
        "top.png",
        doc,
        0,
        &solid(8, 8, [200, 30, 30, 160]),
        "Top",
    );
    assert_eq!(
        plate(doc, 1, CURRENT_AND_BELOW, false),
        plate(doc, 1, ALL, false)
    );
    assert_eq!(plate(doc, 1, ALL, false), flat_pixels(doc));
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------- the cut is not a hide --

/// THE test this file exists for.
///
/// `is_isolated` takes `has_visible_clipped_members`, so HIDING a clipped
/// entry above the target flips its enclosing group from isolated to pass
/// through — and an adjustment inside that group, which was confined to it,
/// then reaches the whole backdrop. That is shipped, deliberate behaviour for
/// a hide (`group_tests::clipping_over_a_pass_through_group_isolates_it`
/// ends on exactly that identity) and it is the WRONG answer for a sampling
/// plate, whose job is to be the picture the user is looking at with some
/// entries left out. The pixels BELOW the target must not move.
///
/// So the cut gates contribution only, and this test is what proves it: the
/// plate agrees with the SCREEN about the layer below, and disagrees with the
/// naive "clear `visible`" implementation, which would read 155.
#[test]
fn cutting_a_clipped_member_does_not_re_isolate_its_group() {
    let dir = TempDir::new().unwrap();
    // [0 Photo] [1 Curves(invert)] [2 G(pass-through)] [3 Texture(clipped)]
    // with Curves inside G. Curves sits BELOW the target so it survives the
    // cut; Texture sits above it and is cut.
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [100, 100, 100, 255]));
    doc = add_layer(
        &dir,
        "adj.png",
        doc,
        0,
        &solid(8, 8, [0, 0, 0, 0]),
        "Curves",
    );
    doc = set_meta(doc, 1, &adjust_meta("invert", "{}"));
    doc = add_layer(&dir, "skin.png", doc, 1, &solid(8, 8, [0, 0, 0, 0]), "Skin");
    let (mut doc, group_idx) = group(doc, &[1, 2], "G");
    assert_eq!(group_idx, 3);
    doc = set_blend(doc, group_idx, PASS_THROUGH);
    doc = add_layer(
        &dir,
        "tex.png",
        doc,
        group_idx,
        &solid(8, 8, [0, 0, 0, 0]),
        "Texture",
    );
    doc = set_clipped(doc, 4, true);

    // On screen the visible clipped member isolates G, so the invert is
    // confined to the group and the photo below reads 100.
    let screen = flat_pixels(doc);
    assert_eq!(pixel(&screen, 8, 4, 4), [100, 100, 100, 255]);

    // The plate for Skin (index 2) cuts Texture, which is above it.
    let sampled = plate(doc, 2, CURRENT_AND_BELOW, false);
    assert_eq!(
        pixel(&sampled, 8, 4, 4),
        [100, 100, 100, 255],
        "the plate must agree with the screen about the pixels BELOW the target: a cut \
         gates contribution, it does not hide"
    );

    // And the proof that this is not vacuous — HIDING the same entry really
    // does move that pixel, to the 155 a `visible`-rewriting implementation
    // would have produced.
    let hidden = unsafe { dr_doc_with_layer_visible(doc, 4, false) };
    assert!(!hidden.is_null());
    assert_eq!(
        pixel(&flat_pixels(hidden), 8, 4, 4),
        [155, 155, 155, 255],
        "hiding it DOES re-isolate the group, which is what the cut must not do"
    );
    unsafe { dr_doc_free(hidden) };
    unsafe { dr_doc_free(doc) };
}

/// A cut member is dropped from its clip run's pixels but KEPT in the run, so
/// the run's emptiness — which is what chooses between the private-buffer
/// path and the direct one — does not change. With an adjustment BASE the two
/// paths differ: an adjustment has no footprint in a transparent buffer, so a
/// non-empty run over one contributes nothing.
#[test]
fn cutting_the_only_clipped_member_keeps_the_run_non_empty() {
    let dir = TempDir::new().unwrap();
    // [0 Photo] [1 Curves(invert), the clip BASE] [2 Texture(clipped)]
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [100, 100, 100, 255]));
    doc = add_layer(
        &dir,
        "adj.png",
        doc,
        0,
        &solid(8, 8, [0, 0, 0, 0]),
        "Curves",
    );
    doc = set_meta(doc, 1, &adjust_meta("invert", "{}"));
    doc = add_layer(
        &dir,
        "tex.png",
        doc,
        1,
        &solid(8, 8, [30, 30, 30, 255]),
        "Texture",
    );
    doc = set_clipped(doc, 2, true);

    // On screen the adjustment is a clip base with a member above it, so it
    // contributes nothing and the photo reads through unchanged.
    assert_eq!(pixel(&flat_pixels(doc), 8, 4, 4), [100, 100, 100, 255]);

    // Cutting Texture must not turn the adjustment back on.
    let sampled = plate(doc, 0, CURRENT_AND_BELOW, false);
    assert_eq!(
        pixel(&sampled, 8, 4, 4),
        [100, 100, 100, 255],
        "a cut member is kept positionally, so its base does not become a bare adjustment"
    );
    unsafe { dr_doc_free(doc) };
}

/// A target inside a group keeps its ANCESTORS: the group's opacity is what
/// says how the target's level meets the picture, and a plate that dropped it
/// would be a level that appears nowhere. The oracle is the same document
/// with only the entry above the target removed.
#[test]
fn an_enclosing_group_still_contributes_to_the_plate() {
    let dir = TempDir::new().unwrap();
    // [0 Photo] [1 Skin] [2 Dust] [3 G] with Skin and Dust inside G at 50%.
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [0, 0, 0, 255]));
    doc = add_layer(
        &dir,
        "skin.png",
        doc,
        0,
        &solid(8, 8, [200, 200, 200, 255]),
        "Skin",
    );
    doc = add_layer(
        &dir,
        "dust.png",
        doc,
        1,
        &solid(8, 8, [0, 0, 255, 255]),
        "Dust",
    );
    let (mut doc, group_idx) = group(doc, &[1, 2], "G");
    assert_eq!(group_idx, 3);
    doc = apply(doc, |d| unsafe {
        dr_doc_with_layer_opacity(d, group_idx, 0.5)
    });

    let sampled = plate(doc, 1, CURRENT_AND_BELOW, false);
    let bare = without_layer(doc, 2);
    assert_eq!(
        sampled,
        flat_pixels(bare),
        "the plate is the picture with the entry above the target gone — group included"
    );
    // 200 at 50 % over black is 100: the group's opacity is in the plate.
    assert_eq!(pixel(&sampled, 8, 4, 4), [100, 100, 100, 255]);
    unsafe { dr_doc_free(bare) };
    unsafe { dr_doc_free(doc) };
}

// --------------------------------------------------- ignore adjustments --

/// An adjustment ABOVE the target is what double-applies, and dropping it is
/// what `ignore_adjustments` is for. Here `all` carries the invert and
/// `all + ignore` does not.
#[test]
fn ignore_adjustments_drops_an_adjustment_above_the_target() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [100, 100, 100, 255]));
    doc = add_layer(
        &dir,
        "adj.png",
        doc,
        0,
        &solid(8, 8, [0, 0, 0, 0]),
        "Curves",
    );
    doc = set_meta(doc, 1, &adjust_meta("invert", "{}"));

    assert_eq!(
        pixel(&plate(doc, 0, ALL, false), 8, 4, 4),
        [155, 155, 155, 255],
        "plain all-layers sampling carries the grade"
    );
    assert_eq!(
        pixel(&plate(doc, 0, ALL, true), 8, 4, 4),
        [100, 100, 100, 255],
        "ignore_adjustments samples the photograph the grade is applied to"
    );
    unsafe { dr_doc_free(doc) };
}

/// `ignore_adjustments` is a provable IDENTITY in the two modes that already
/// exclude everything above the target. An adjustment transforms only what is
/// composited below it, so one below the target can never reach the target's
/// own pixels — dropping it would sample a picture that exists nowhere.
#[test]
fn ignore_adjustments_is_an_identity_below_the_target() {
    let dir = TempDir::new().unwrap();
    // The adjustment is BELOW the target: it grades the photo, and the
    // target's own level sits on the result.
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [100, 100, 100, 255]));
    doc = add_layer(
        &dir,
        "adj.png",
        doc,
        0,
        &solid(8, 8, [0, 0, 0, 0]),
        "Curves",
    );
    doc = set_meta(doc, 1, &adjust_meta("invert", "{}"));
    doc = add_layer(&dir, "skin.png", doc, 1, &solid(8, 8, [0, 0, 0, 0]), "Skin");

    for mode in [CURRENT, CURRENT_AND_BELOW] {
        assert_eq!(
            plate(doc, 2, mode, true),
            plate(doc, 2, mode, false),
            "ignore_adjustments changes nothing in mode {mode}: every entry above the \
             target is already cut, and one below cannot reach it"
        );
    }
    // It is not vacuous: the grade IS in the current-and-below plate, because
    // it is genuinely part of the picture beneath the target.
    assert_eq!(
        pixel(&plate(doc, 2, CURRENT_AND_BELOW, false), 8, 4, 4),
        [155, 155, 155, 255]
    );
    unsafe { dr_doc_free(doc) };
}

/// The stated BOUNDARY: only an adjustment ENTRY is dropped. A plain raster
/// layer in a tone blend mode — the grey Soft Light layer of every retouching
/// stack — is ordinary content and stays in, so a caller is never sampling a
/// document nobody has seen.
#[test]
fn ignore_adjustments_keeps_a_tone_blended_raster_layer() {
    let dir = TempDir::new().unwrap();
    const MULTIPLY: i32 = 1;
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [200, 200, 200, 255]));
    doc = add_layer(
        &dir,
        "dodge.png",
        doc,
        0,
        &solid(8, 8, [128, 128, 128, 255]),
        "D&B",
    );
    doc = set_blend(doc, 1, MULTIPLY);

    let kept = plate(doc, 0, ALL, true);
    assert_eq!(
        kept,
        plate(doc, 0, ALL, false),
        "a blend mode is content, not an adjustment entry: ignore_adjustments leaves it"
    );
    assert_eq!(
        pixel(&kept, 8, 4, 4),
        [100, 100, 100, 255],
        "and the multiply really is in the plate"
    );
    unsafe { dr_doc_free(doc) };
}

/// A GROUP's meta is the host's alone and the compositor never interprets it,
/// so a group cannot be an adjustment — here or anywhere. Pinned because
/// `Adjustment::from_meta` alone would say otherwise.
#[test]
fn a_group_whose_meta_parses_as_an_adjustment_is_not_cut() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [100, 100, 100, 255]));
    doc = add_layer(
        &dir,
        "inner.png",
        doc,
        0,
        &solid(8, 8, [40, 40, 40, 255]),
        "Inner",
    );
    let (mut doc, group_idx) = group(doc, &[1], "G");
    doc = set_meta(doc, group_idx, &adjust_meta("invert", "{}"));

    assert_eq!(
        plate(doc, 0, ALL, true),
        plate(doc, 0, ALL, false),
        "a group is never an adjustment entry, so ignore_adjustments does not cut it"
    );
    assert_eq!(
        pixel(&plate(doc, 0, ALL, true), 8, 4, 4),
        [40, 40, 40, 255],
        "the group's own child still paints"
    );
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------------- refusals --

/// An unknown mode code is a plain refusal with no message — a domain
/// refusal, never a silent fallback to a mode the caller did not ask for.
#[test]
fn an_unknown_sampling_mode_refuses_without_a_message() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &solid(4, 4, [10, 20, 30, 255]));
    let (pixels, message) = try_plate(doc, 0, 3, false);
    assert!(pixels.is_none());
    assert!(message.is_none(), "an unknown code is a domain refusal");
    unsafe { dr_doc_free(doc) };
}

/// NULL safety and the index rule. `current` needs a raster layer, because it
/// answers with that layer's pixels; the composite modes do not, because they
/// composite the document — a group index simply names a position in it.
#[test]
fn a_null_document_and_a_bad_index_refuse_cleanly() {
    let (pixels, message) = try_plate(ptr::null(), 0, ALL, false);
    assert!(pixels.is_none() && message.is_none());

    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "photo.png", &solid(4, 4, [10, 20, 30, 255]));
    let (pixels, message) = try_plate(doc, 99, CURRENT, false);
    assert!(
        pixels.is_none(),
        "current needs a raster layer at that index"
    );
    assert!(message.is_none());
    // A composite mode past the end cuts nothing and answers the projection.
    assert_eq!(plate(doc, 99, ALL, false), flat_pixels(doc));
    unsafe { dr_doc_free(doc) };
}

/// A GROUP index refuses in `current` — a group has no pixels of its own —
/// and composites normally in the other two.
#[test]
fn a_group_index_refuses_only_the_current_layer_mode() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [100, 100, 100, 255]));
    doc = add_layer(
        &dir,
        "inner.png",
        doc,
        0,
        &solid(8, 8, [40, 40, 40, 255]),
        "Inner",
    );
    let (doc, group_idx) = group(doc, &[1], "G");

    let (pixels, message) = try_plate(doc, group_idx, CURRENT, false);
    assert!(
        pixels.is_none(),
        "a group has no pixels of its own to sample"
    );
    assert!(message.is_none());
    assert_eq!(plate(doc, group_idx, ALL, false), flat_pixels(doc));
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------------- identity --

/// The cut must be INVISIBLE to every other render. A document sampled in
/// every mode still projects, exports and thumbnails exactly as it did — the
/// `Cut::NONE` path is the statements it always ran.
#[test]
fn sampling_never_changes_the_documents_own_projection() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "photo.png", &opaque_pattern(16, 12));
    doc = add_layer(
        &dir,
        "adj.png",
        doc,
        0,
        &solid(16, 12, [0, 0, 0, 0]),
        "Curves",
    );
    doc = set_meta(doc, 1, &adjust_meta("invert", "{}"));
    doc = add_layer(
        &dir,
        "top.png",
        doc,
        1,
        &solid(16, 12, [9, 200, 9, 90]),
        "Top",
    );

    let before = flat_pixels(doc);
    for mode in [CURRENT, CURRENT_AND_BELOW, ALL] {
        for ignore in [false, true] {
            let _ = try_plate(doc, 0, mode, ignore);
            let _ = try_plate(doc, 2, mode, ignore);
        }
    }
    assert_eq!(
        flat_pixels(doc),
        before,
        "building a plate is a read: the document's own projection is unchanged"
    );
    unsafe { dr_doc_free(doc) };
}

/// The plate is a sampling source, so it must carry the document's own
/// numbers with no conversion. A one-layer opaque document's `all` plate is
/// its pixels, byte for byte.
#[test]
fn the_plate_carries_the_documents_own_numbers() {
    let dir = TempDir::new().unwrap();
    let source = opaque_pattern(12, 9);
    let doc = doc_from(&dir, "photo.png", &source);
    let sampled = plate(doc, 0, ALL, false);
    assert_eq!(
        sampled,
        source.as_raw().as_slice(),
        "no conversion anywhere: a sampled colour converts nowhere"
    );
    assert_eq!(plate(doc, 0, CURRENT, false), sampled);
    assert_eq!(plate(doc, 0, CURRENT_AND_BELOW, false), sampled);
    unsafe { dr_doc_free(doc) };
}

/// An INVISIBLE layer above the target is already contributing nothing, so
/// cutting it changes nothing — the cut and the hide agree wherever they are
/// asking the same question. (Where they disagree is the isolation test
/// above, which is the whole point of having both.)
#[test]
fn cutting_an_already_hidden_layer_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [100, 100, 100, 255]));
    doc = add_layer(
        &dir,
        "top.png",
        doc,
        0,
        &solid(8, 8, [240, 10, 10, 255]),
        "Top",
    );
    doc = set_visible(doc, 1, false);
    assert_eq!(
        plate(doc, 0, CURRENT_AND_BELOW, false),
        plate(doc, 0, ALL, false)
    );
    assert_eq!(plate(doc, 0, ALL, false), flat_pixels(doc));
    unsafe { dr_doc_free(doc) };
}

/// Depth: a cut over a nest keeps every level's own structure. The target
/// sits two groups deep, and the plate is the picture with the entry above it
/// gone — ancestors and all.
#[test]
fn the_cut_survives_nesting() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "photo.png", &solid(8, 8, [10, 10, 10, 255]));
    doc = add_layer(
        &dir,
        "skin.png",
        doc,
        0,
        &solid(8, 8, [180, 180, 180, 255]),
        "Skin",
    );
    doc = add_layer(
        &dir,
        "dust.png",
        doc,
        1,
        &solid(8, 8, [0, 0, 200, 255]),
        "Dust",
    );
    let (doc, inner) = group(doc, &[1, 2], "Inner");
    let (mut doc, outer) = group(doc, &[inner], "Outer");
    doc = apply(doc, |d| unsafe { dr_doc_with_layer_opacity(d, outer, 0.5) });

    let sampled = plate(doc, 1, CURRENT_AND_BELOW, false);
    let bare = without_layer(doc, 2);
    assert_eq!(sampled, flat_pixels(bare));
    unsafe { dr_doc_free(bare) };
    unsafe { dr_doc_free(doc) };
}
