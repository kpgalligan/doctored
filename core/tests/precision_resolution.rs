//! Phase 12E-1b: `set_resolution` at high depth — PPI ONLY.
//!
//! The claim this file pins is the one P7-26 words as "PPI changes must not
//! resample pixels", and it is checked the strongest way there is: every
//! raster's sample buffer is POINTER-equal before and after, at 16 bits and
//! at float. A comparison of the samples themselves would pass on a copy; a
//! pointer comparison passes only when the allocation was SHARED.

use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_image_storage_identity;
use doctored_core::ffi_precision_metadata::dr_precision_doc_set_resolution;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// Each raster record's STORAGE IDENTITY, in postorder.
fn addresses(document: &PrecisionDocument) -> Vec<usize> {
    (0..document.layer_count())
        .filter_map(|i| document.layer_source(i))
        .map(|image| unsafe { dr_precision_image_storage_identity(image) as usize })
        .collect()
}

fn fixture() -> PrecisionDocument {
    let image = |code: u16| {
        PrecisionImage::from_rgba16(
            2,
            2,
            vec![[code, code / 2, code / 4, 65535]; 4],
            IccProfile::srgb(),
            None,
        )
        .expect("an image")
    };
    let inner = Layer::raster("Inner", image(40000), (0, 0), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Group", vec![inner], 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        2,
        2,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution { x: 300.0, y: 300.0 },
        vec![
            Layer::raster("Base", image(20000), (0, 0), 1.0, true).unwrap(),
            group,
        ],
    )
    .expect("a document")
}

/// A change or the no-op; either way no error is set.
fn set(document: &PrecisionDocument, x: f32, y: f32) -> Option<Box<PrecisionDocument>> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_resolution(document, x, y, &mut err) };
    assert!(err.is_null(), "{x}x{y} is not an error");
    (!out.is_null()).then(|| unsafe { Box::from_raw(out) })
}

/// A refusal: NULL, with the sentence that names it.
fn refusal(document: &PrecisionDocument, x: f32, y: f32) -> String {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_resolution(document, x, y, &mut err) };
    assert!(out.is_null(), "{x}x{y} is refused");
    assert!(!err.is_null(), "{x}x{y} names its refusal");
    let message = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    message
}

#[test]
fn set_resolution_shares_every_u16_buffer() {
    let document = fixture();
    let before = addresses(&document);
    assert_eq!(before.len(), 2, "two raster records, one of them nested");

    let changed = set(&document, 144.0, 144.0).expect("300 -> 144 is a change");
    assert_eq!(changed.resolution().x, 144.0);
    assert_eq!(changed.resolution().y, 144.0);
    assert_eq!(
        addresses(&changed),
        before,
        "every raster's samples are the very allocation they were"
    );
    assert_eq!(
        changed.dimensions(),
        document.dimensions(),
        "and the canvas is untouched"
    );
    assert_eq!(
        document.resolution().x,
        300.0,
        "the input document is unchanged: the op is pure"
    );

    // The exact echo is the no-op, so a host never mints an undo step for it.
    assert!(
        set(&changed, 144.0, 144.0).is_none(),
        "setting the resolution it already carries changes nothing"
    );
    // …and so is a non-finite or non-positive request.
    for (x, y) in [(0.0, 144.0), (144.0, -1.0), (f32::NAN, 144.0)] {
        assert_eq!(
            refusal(&changed, x, y),
            "print resolution must be finite and positive"
        );
    }
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_set_resolution(ptr::null(), 144.0, 144.0, &mut err) }.is_null(),
        "and a NULL document answers NULL"
    );
    assert!(err.is_null(), "without an error");
}

#[test]
fn set_resolution_shares_every_float_buffer() {
    let document =
        PrecisionDocument::open_preserving("tests/fixtures/float_11f/scene.tiff", LIMIT, 0, None)
            .expect("the float fixture opens");
    assert!(document.is_float());
    let before = addresses(&document);
    assert!(!before.is_empty(), "the fixture holds a raster");

    let changed = set(&document, 144.0, 144.0).expect("the resolution moves");
    assert_eq!(changed.resolution().x, 144.0);
    assert_eq!(
        addresses(&changed),
        before,
        "a float document's f32 samples are shared too"
    );
    assert!(changed.is_float(), "and it is still a float document");
}

#[test]
fn the_new_resolution_survives_the_native_round_trip() {
    let document = fixture();
    let changed = set(&document, 144.0, 72.0).expect("the resolution moves");
    let bytes = changed.encode_native(LIMIT, 0, None).expect("bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(reopened.resolution().x, 144.0);
    assert_eq!(reopened.resolution().y, 72.0);
    // The pixels came back identical too, which is what "PPI only" means
    // across a save: nothing resampled on the way out or the way in.
    for i in 0..reopened.layer_count() {
        if let (Some(a), Some(b)) = (reopened.layer_source(i), changed.layer_source(i)) {
            assert_eq!(a.to_rgba16().ok(), b.to_rgba16().ok(), "record {i}");
        }
    }
}
