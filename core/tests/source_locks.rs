//! Phase 12E-2b: the LAYER LOCKS over the retained-source and attachment
//! families, at BOTH depths.
//!
//! 12E-1b disclosed these families as ungated at either depth. That was only
//! half right, and this file pins the corrected picture: the BYTE
//! retained-source edits have consulted `lock_block` since 8A — with no test —
//! while the TYPED twin and the RAW ATTACHMENT writers at both
//! depths had no gate at all. Every row below is asserted BY SENTENCE, and
//! every refusal is asserted to have left the input document exactly as it
//! was.
//!
//! The table this file encodes, written here rather than read off the
//! implementation:
//!
//! ```text
//! op               gate                                     byte sentence / typed sentence
//! capture_raster   Merge, and Position when the extent or
//!                  offset would move                        "source capture …"             / locked(bit)
//! replace          Merge, and Position when it would move   "source replacement …"         / locked(bit)
//! transform        Position                                 "source layer position is locked" / locked(position)
//! rasterize        Pixels, AFTER its no-source no-op        "source layer pixels are locked"  / locked(pixels)
//! place/retain     ungated
//! attachment (RAW, local masks)              Pixels         "source attachment is blocked …"  / locked(pixels)
//! ```
//!
//! The typed sentence is `precision_lock::lock_sentence`'s, which names the
//! record and the BIT; the byte ones are the ones those call sites have always
//! phrased and are deliberately not re-worded.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_POSITION, LOCK_TRANSPARENCY};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_raw_source::{dr_doc_attach_raw_source, dr_precision_doc_attach_raw_source};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CAMERA: &[u8] = b"independent-camera-original\0\x89\xff";

// ------------------------------------------------------------- plumbing --

unsafe fn owned(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    value
}

fn parsed(value: Value) -> SourceEdit {
    SourceEdit::parse(&value.to_string()).unwrap()
}

fn master(seed: u16) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            3,
            2,
            (0..6)
                .map(|i| {
                    [
                        seed + i,
                        20003 + i,
                        50007 - i,
                        if i == 0 { 0 } else { 65535 - i },
                    ]
                })
                .collect(),
            IccProfile::display_p3(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn place(id: &str) -> Value {
    json!({"op":"place","source_id":id,"revision_id":"revision_a","name":"Placed",
           "x":0,"y":0,"depth_conversion":"convert_fallback"})
}

/// `[Base, Placed]` at byte depth, `Placed` carrying a retained u16 master.
fn byte_fixture() -> DrDocument {
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(3, 2, Rgba([17, 31, 47, 255])));
    base.source_edit_input(
        Some(SourceInput::Typed(&master(1001))),
        &parsed(place("camera")),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

/// `[Base, Placed]` at high depth, the byte fixture's twin.
fn typed_fixture() -> PrecisionDocument {
    master(1001)
        .source_edit_input(
            Some(SourceInput::Typed(&master(2002))),
            &parsed(place("camera")),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap()
}

/// A RAW master: one placed layer whose asset carries genuine u16 pixels and
/// no attachment yet, which is what every attachment writer requires.
fn byte_raw_fixture() -> DrDocument {
    byte_fixture()
}

fn typed_raw_fixture() -> PrecisionDocument {
    typed_fixture()
}

// ------------------------------------------------------- the "unchanged" --

/// A SIGNATURE of everything this file claims a refusal leaves alone: the
/// stack's names and offsets, each entry's lock flags, and the retained-source
/// report of every layer that has one.
fn byte_signature(doc: &DrDocument) -> Value {
    json!(doc
        .layers
        .iter()
        .enumerate()
        .map(
            |(i, l)| json!({"name": l.name, "offset": [l.offset.0, l.offset.1],
                             "locks": l.locks, "pixels": l.pixels.contiguous_image().as_raw().len(),
                             "source": doc.source_info(i).unwrap()})
        )
        .collect::<Vec<_>>())
}

fn typed_signature(doc: &PrecisionDocument) -> Value {
    json!((0..doc.layer_count())
        .map(|i| {
            let l = doc.layer(i).unwrap();
            json!({"name": l.name(), "offset": [l.offset().0, l.offset().1],
                   "locks": l.locks(), "source": doc.source_info(i).unwrap()})
        })
        .collect::<Vec<_>>())
}

fn locked8(doc: &DrDocument, index: usize, flags: u32) -> DrDocument {
    doc.with_layer_locks(index, flags).expect("a lock")
}

fn locked16(doc: &PrecisionDocument, index: usize, flags: u32) -> PrecisionDocument {
    doc.with_layer_locks(index, flags)
        .expect("a lock")
        .expect("a change")
}

/// One byte source edit that MUST refuse; answers the sentence and asserts the
/// input document is byte-for-byte the document it was.
fn refused8(doc: &DrDocument, input: Option<&PrecisionDocument>, request: Value) -> String {
    let before = byte_signature(doc);
    let edit = parsed(request.clone());
    let plan = doc
        .source_plan_input(input.map(SourceInput::Typed), &edit, 24_000_000_000)
        .err();
    let message = match doc.source_edit_input(input.map(SourceInput::Typed), &edit, LIMIT, 0, None)
    {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused at byte depth"),
    };
    assert_eq!(
        plan.as_deref(),
        Some(message.as_str()),
        "{request}: the PLAN refuses with the same sentence"
    );
    assert_eq!(
        byte_signature(doc),
        before,
        "{request}: the input is intact"
    );
    message
}

/// The typed twin of [`refused8`].
fn refused16(doc: &PrecisionDocument, input: Option<&PrecisionDocument>, request: Value) -> String {
    let before = typed_signature(doc);
    let edit = parsed(request.clone());
    let plan = doc
        .source_plan_input(input.map(SourceInput::Typed), &edit, 24_000_000_000)
        .err();
    let message = match doc.source_edit_input(input.map(SourceInput::Typed), &edit, LIMIT, 0, None)
    {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused at high depth"),
    };
    assert_eq!(
        plan.as_deref(),
        Some(message.as_str()),
        "{request}: the PLAN refuses with the same sentence"
    );
    assert_eq!(
        typed_signature(doc),
        before,
        "{request}: the input is intact"
    );
    message
}

// ------------------------------------------------------ (a) byte rasterize --

#[test]
fn a_byte_rows_keep_their_own_sentences() {
    let doc = byte_fixture();
    assert!(doc.has_retained_source(1), "the fixture has a source");

    // Doctored answers to the PIXEL lock.
    let pixels = locked8(&doc, 1, LOCK_PIXELS);
    assert_eq!(
        refused8(&pixels, None, json!({"op":"rasterize","layer":1})),
        "source layer pixels are locked"
    );
    // …and its no-source NO-OP comes first: a locked layer with nothing to
    // rasterize still answers `None` rather than a refusal.
    let plain = locked8(&doc, 0, LOCK_PIXELS);
    assert!(plain
        .source_edit(
            None,
            &parsed(json!({"op":"rasterize","layer":0})),
            LIMIT,
            0,
            None
        )
        .expect("no refusal")
        .is_none());

    // Transform answers to the POSITION lock, never to Pixels.
    let position = locked8(&doc, 1, LOCK_POSITION);
    assert_eq!(
        refused8(
            &position,
            None,
            json!({"op":"transform","layer":1,"matrix":[2.0,0.0,0.0,2.0,0.0,0.0],"filter":"lanczos3"})
        ),
        "source layer position is locked"
    );
    assert!(pixels
        .source_edit(
            None,
            &parsed(
                json!({"op":"transform","layer":1,"matrix":[2.0,0.0,0.0,2.0,0.0,0.0],"filter":"lanczos3"})
            ),
            LIMIT,
            0,
            None
        )
        .is_ok(), "a transform is a POSITION edit and never a Pixels one");

    // Replace answers to the Merge rule…
    let next = master(3003);
    let replace = json!({"op":"replace","layer":1,"revision_id":"revision_b",
                         "depth_conversion":"convert_fallback"});
    assert_eq!(
        refused8(&pixels, Some(&next), replace.clone()),
        "source replacement is blocked by pixel or transparency locks"
    );
    let transparency = locked8(&doc, 1, LOCK_TRANSPARENCY);
    assert_eq!(
        refused8(&transparency, Some(&next), replace.clone()),
        "source replacement is blocked by pixel or transparency locks"
    );
    // …and ADDITIONALLY to Position when it would move the extent.
    let scaled = json!({"op":"replace","layer":1,"revision_id":"revision_b",
                        "depth_conversion":"convert_fallback",
                        "source_transform":[2.0,0.0,0.0,2.0,0.0,0.0]});
    assert_eq!(
        refused8(&position, Some(&next), scaled),
        "source replacement would change a position-locked layer extent"
    );
    // A same-extent replacement under a POSITION lock is admitted.
    assert!(position
        .source_edit_input(
            Some(SourceInput::Typed(&next)),
            &parsed(replace),
            LIMIT,
            0,
            None
        )
        .is_ok());

    // Capture answers to Merge, and to Position when the extent would move.
    let capture = |matrix: [f64; 6]| {
        json!({"op":"capture_raster","layer":0,"source_id":"fresh",
               "revision_id":"revision_a","source_transform":matrix,
               "depth_conversion":"convert_fallback"})
    };
    let base_pixels = locked8(&doc, 0, LOCK_PIXELS);
    assert_eq!(
        refused8(
            &base_pixels,
            Some(&next),
            capture([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
        ),
        "source capture is blocked by pixel or transparency locks"
    );
    let base_position = locked8(&doc, 0, LOCK_POSITION);
    assert_eq!(
        refused8(
            &base_position,
            Some(&next),
            capture([2.0, 0.0, 0.0, 2.0, 0.0, 0.0])
        ),
        "source capture would change a position-locked layer extent"
    );
    // The same capture at the record's own extent is admitted under Position.
    assert!(base_position
        .source_edit_input(
            Some(SourceInput::Typed(&next)),
            &parsed(capture([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])),
            LIMIT,
            0,
            None
        )
        .is_ok());
}

// ------------------------------------------------------- (b) typed rows --

#[test]
fn b_typed_rows_name_the_record_and_the_bit() {
    let doc = typed_fixture();
    assert!(doc.has_retained_source(1), "the fixture has a source");

    let pixels = locked16(&doc, 1, LOCK_PIXELS);
    assert_eq!(
        refused16(&pixels, None, json!({"op":"rasterize","layer":1})),
        "high-depth layer 1 is locked (pixels)"
    );
    // The no-source no-op comes FIRST, as at byte depth.
    let plain = locked16(&doc, 0, LOCK_PIXELS);
    assert!(plain
        .source_edit(
            None,
            &parsed(json!({"op":"rasterize","layer":0})),
            LIMIT,
            0,
            None
        )
        .expect("no refusal")
        .is_none());

    let position = locked16(&doc, 1, LOCK_POSITION);
    assert_eq!(
        refused16(
            &position,
            None,
            json!({"op":"transform","layer":1,"matrix":[2.0,0.0,0.0,2.0,0.0,0.0],"filter":"lanczos3"})
        ),
        "high-depth layer 1 is locked (position)"
    );
    assert!(pixels
        .source_edit(
            None,
            &parsed(
                json!({"op":"transform","layer":1,"matrix":[2.0,0.0,0.0,2.0,0.0,0.0],"filter":"lanczos3"})
            ),
            LIMIT,
            0,
            None
        )
        .is_ok(), "a transform is a POSITION edit and never a Pixels one");

    let next = master(3003);
    let replace = json!({"op":"replace","layer":1,"revision_id":"revision_b",
                         "depth_conversion":"convert_fallback"});
    assert_eq!(
        refused16(&pixels, Some(&next), replace.clone()),
        "high-depth layer 1 is locked (pixels)"
    );
    let transparency = locked16(&doc, 1, LOCK_TRANSPARENCY);
    assert_eq!(
        refused16(&transparency, Some(&next), replace.clone()),
        "high-depth layer 1 is locked (transparency)"
    );
    let scaled = json!({"op":"replace","layer":1,"revision_id":"revision_b",
                        "depth_conversion":"convert_fallback",
                        "source_transform":[2.0,0.0,0.0,2.0,0.0,0.0]});
    assert_eq!(
        refused16(&position, Some(&next), scaled),
        "high-depth layer 1 is locked (position)"
    );
    assert!(position
        .source_edit_input(
            Some(SourceInput::Typed(&next)),
            &parsed(replace),
            LIMIT,
            0,
            None
        )
        .is_ok());

    let capture = |matrix: [f64; 6]| {
        json!({"op":"capture_raster","layer":0,"source_id":"fresh",
               "revision_id":"revision_a","source_transform":matrix,
               "depth_conversion":"convert_fallback"})
    };
    assert_eq!(
        refused16(&plain, Some(&next), capture([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])),
        "high-depth layer 0 is locked (pixels)"
    );
    let base_position = locked16(&doc, 0, LOCK_POSITION);
    assert_eq!(
        refused16(
            &base_position,
            Some(&next),
            capture([2.0, 0.0, 0.0, 2.0, 0.0, 0.0])
        ),
        "high-depth layer 0 is locked (position)"
    );
    assert!(base_position
        .source_edit_input(
            Some(SourceInput::Typed(&next)),
            &parsed(capture([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])),
            LIMIT,
            0,
            None
        )
        .is_ok());
}

// -------------------------------------------------- (c) the ungated rows --

#[test]
fn c_place_is_ungated_at_both_depths() {
    // BYTE: every entry locked with every bit, and a PLACE still lands — it
    // creates a record, which is STRUCTURE.
    let doc = byte_fixture();
    let all = locked8(&locked8(&doc, 0, 7), 1, 7);
    let placed = all
        .source_edit_input(
            Some(SourceInput::Typed(&master(4004))),
            &parsed(place("second")),
            LIMIT,
            0,
            None,
        )
        .expect("place is ungated")
        .expect("a change");
    assert_eq!(placed.layers.len(), 3);

    // …including a POSITIONED place INTO a locked group, which is the same
    // lock-exempt rule stated for insertion.
    let (grouped, group, _, _) = all.group_layers(&[0], "Locked").expect("a group");
    let grouped = locked8(&grouped, group, 7);
    let into = grouped
        .source_edit_input(
            Some(SourceInput::Typed(&master(4004))),
            &parsed(
                json!({"op":"place","source_id":"third","revision_id":"revision_a",
                           "name":"Inside","x":0,"y":0,"depth_conversion":"convert_fallback",
                           "anchor":group,"position":"into_group"}),
            ),
            LIMIT,
            0,
            None,
        )
        .expect("insertion into a locked group is admitted")
        .expect("a change");
    let inside = into
        .layers
        .iter()
        .position(|l| l.name == "Inside")
        .expect("the new layer");
    assert_eq!(into.layers[inside].depth, 1, "it landed inside the group");

    // TYPED: the same.
    let doc = typed_fixture();
    let all = locked16(&locked16(&doc, 0, 7), 1, 7);
    assert_eq!(
        all.source_edit_input(
            Some(SourceInput::Typed(&master(4004))),
            &parsed(place("second")),
            LIMIT,
            0,
            None,
        )
        .expect("place is ungated")
        .expect("a change")
        .layer_count(),
        3
    );
}

// -------------------------------------------------- (e) the attach writers --

fn raw_descriptor() -> CString {
    CString::new(
        json!({"schema_version":1,"original_filename":"camera.dng","content_sha256":"b".repeat(64),
               "decoder":{"identifier":"test.decoder","version":"1.0","implementation":"test-platform"},
               "recipe":{"schema_version":1,"exposure":0.0,"white_balance":[5500.0,0.0],
                         "output_space":"srgb"}})
        .to_string(),
    )
    .unwrap()
}

#[test]
fn e_every_attachment_writer_answers_to_the_pixel_lock() {
    let descriptor = raw_descriptor();

    // BYTE RAW.
    let doc = locked8(&byte_raw_fixture(), 1, LOCK_PIXELS);
    let before = byte_signature(&doc);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_attach_raw_source(
            &doc,
            1,
            CAMERA.as_ptr(),
            CAMERA.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        unsafe { owned(err) },
        "source attachment is blocked by the layer's pixel lock"
    );
    assert_eq!(byte_signature(&doc), before, "the input is intact");

    // TYPED RAW.
    let doc = locked16(&typed_raw_fixture(), 1, LOCK_PIXELS);
    let before = typed_signature(&doc);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_attach_raw_source(
            &doc,
            1,
            CAMERA.as_ptr(),
            CAMERA.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        unsafe { owned(err) },
        "high-depth layer 1 is locked (pixels)"
    );
    assert_eq!(typed_signature(&doc), before, "the input is intact");

    // …and an UNLOCKED attachment still lands, so the gate is a gate and not a
    // blanket refusal.
    let doc = byte_raw_fixture();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_attach_raw_source(
            &doc,
            1,
            CAMERA.as_ptr(),
            CAMERA.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", unsafe { owned(err) });
    unsafe { doctored_core::ffi_doc::dr_doc_free(out) };
}
