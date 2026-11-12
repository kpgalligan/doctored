//! The RAW-local counts: sixteen locals, four of them with a camera
//! variant, sixteen masks on one attachment, the 1 GiB combined resource cap,
//! the arithmetic of how many locals fit beside an original, and `.rz` word
//! 79 at both depths.
//!
//! Declared before the first run: `fitting_locals` is 11 for the D850 file
//! (45,441,024 px, 55,196,715 B), 8 for the A7R IV file (9504×6336 px,
//! 61,983,744 B) and 16 for 24 MP beside 256 MiB; 17 locals, 5 variants, 17
//! attachment masks and more than 1 GiB of resources refuse by sentence;
//! sixteen masks save and reopen bit-exact at both depths with word 79 right
//! after the identity word, four masks write no word, and a file whose word
//! and sources disagree refuses in both directions.
use crate::raw_local_numeric::{image, mask_edit, Mask};
use crate::raw_local_tone::try_develop;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_raw_local::*;
use doctored_core::ffi_raw_source::{dr_doc_attach_raw_source, dr_precision_doc_attach_raw_source};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::raw_local_limits::fitting_locals;
use doctored_core::raw_local_mask::RawLocalMask;
use doctored_core::raw_locals_native::claim;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use doctored_core::source_attachment::ResourceCapacity;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const ORIGINAL: &[u8] = b"opaque original camera bytes for the capacity test";
const MISSING_WORD: &str =
    "RAW sources with more than four local masks or over 512 MiB of resources require DRDC 79";

fn text(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    s
}
fn c(v: Value) -> CString {
    CString::new(v.to_string()).unwrap()
}

#[test]
fn locals_that_fit_beside_an_original_follow_the_stated_figures() {
    assert_eq!(fitting_locals(45_441_024, 55_196_715), 11, "D850");
    assert_eq!(fitting_locals(9504 * 6336, 61_983_744), 8, "A7R IV");
    assert_eq!(fitting_locals(24_000_000, 256 * 1024 * 1024), 16, "24 MP");
    assert_eq!(fitting_locals(6000 * 4000, 0), 16, "capped at sixteen");
    assert_eq!(
        fitting_locals(1, (1 << 30) + 1),
        0,
        "an original over the cap"
    );
}

#[test]
fn the_word_79_claim_is_more_than_four_or_more_than_512_mib() {
    let half = 512 * 1024 * 1024;
    assert!(!claim(half, 4));
    assert!(claim(half + 1, 4));
    assert!(claim(0, 5));
    assert!(!claim(0, 0));
}

#[test]
fn more_than_a_gibibyte_of_resources_refuses() {
    let mut capacity = ResourceCapacity::default();
    for _ in 0..4 {
        capacity.add_length(256 * 1024 * 1024).unwrap();
    }
    assert_eq!(
        capacity.add_length(1).unwrap_err(),
        "retained RAW and source resources exceed the 1 GiB aggregate limit"
    );
}

#[test]
fn seventeen_locals_and_a_fifth_variant_refuse() {
    let (w, h) = (4, 3);
    let pixels: Vec<[u16; 4]> = (0..12).map(|i| [1000 * i, 2000, 3000, 65535]).collect();
    let base = image(w, h, &pixels, &IccProfile::srgb());
    let masks: Vec<Mask> = (0..17)
        .map(|_| mask_edit(None, w, h, json!({"op":"full"})).unwrap())
        .collect();
    let refs: Vec<&Mask> = masks.iter().collect();
    let setting = json!({"enabled":true,"density":1,"feather":0,"tone":{"contrast":10}});
    let error = try_develop(&base, &[None; 17], &refs, json!(vec![setting.clone(); 17]))
        .err()
        .expect("17 locals refused");
    assert!(error.contains("zero to sixteen"), "{error}");
    assert!(try_develop(
        &base,
        &[None; 16],
        &refs[..16],
        json!(vec![setting.clone(); 16])
    )
    .unwrap()
    .is_some());
    let variants: Vec<Option<&PrecisionDocument>> =
        (0..16).map(|i| (i < 5).then_some(&*base)).collect();
    let error = try_develop(
        &base,
        &variants,
        &refs[..16],
        json!(vec![setting.clone(); 16]),
    )
    .err()
    .expect("5 variants refused");
    assert_eq!(
        error,
        "local development input count or settings exceed the supported bounds"
    );
    assert!(try_develop(&base, &variants[1..], &refs[..15], json!(vec![setting; 15])).is_ok());
}

fn master() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            3,
            2,
            (0..6)
                .map(|i| [10001 + i, 20003 + i, 30007 + i, 65535])
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
fn masks(count: usize) -> Vec<RawLocalMask> {
    (0..count)
        .map(|i| {
            let command = c(json!({"op":"linear","from":[0.1 * i as f64, 0],"to":[3, 2]}));
            let mut err = ptr::null_mut();
            let p = unsafe {
                dr_raw_local_mask_edit(
                    ptr::null(),
                    3,
                    2,
                    command.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(err.is_null(), "{}", text(err));
            *unsafe { Box::from_raw(p) }
        })
        .collect()
}
fn descriptor(masks: &[&RawLocalMask]) -> Value {
    json!({"schema_version":1,"kind":"raw_local_masks",
        "state":{"schema_version":1,"coordinates":"oriented-source-pixel-centers-v1",
                 "width":3,"height":2,
                 "nodes":(0..masks.len()).map(|i|format!("node{i}")).collect::<Vec<_>>()},
        "resources":masks.iter().enumerate().map(|(i,m)|json!({"id":format!("node{i}"),
            "media_type":"com.rasterize.raw-local-mask","sha256":"a".repeat(64),
            "byte_length":m.encoded().len()})).collect::<Vec<_>>()})
}
enum Doc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}
impl Doc {
    fn fresh(typed: bool) -> Self {
        let source = master();
        let request = SourceEdit::parse(
            &json!({"op":"capture_raster","layer":0,"source_id":"camera","revision_id":"v1",
                    "source_transform":[1,0,0,1,0,0],"depth_conversion":"convert_fallback"})
            .to_string(),
        )
        .unwrap();
        let input = Some(SourceInput::Typed(&source));
        let raw = c(json!({"schema_version":1,"original_filename":"fixture.dng",
            "content_sha256":"b".repeat(64),
            "decoder":{"identifier":"independent","version":"1","implementation":"core-only-fixture"},
            "recipe":{"schema_version":3,"tint":11.999950408935547_f64}}));
        let mut err = ptr::null_mut();
        let out = if typed {
            let d = master()
                .source_edit_input(input, &request, LIMIT, 0, None)
                .unwrap()
                .unwrap();
            let p = unsafe {
                dr_precision_doc_attach_raw_source(
                    &d,
                    0,
                    ORIGINAL.as_ptr(),
                    ORIGINAL.len(),
                    raw.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(!p.is_null(), "{}", text(err));
            Self::Typed(*unsafe { Box::from_raw(p) })
        } else {
            let d = DrDocument::from_pixels(RgbaImage::from_pixel(3, 2, Rgba([0, 0, 0, 0])))
                .source_edit_input(input, &request, LIMIT, 0, None)
                .unwrap()
                .unwrap();
            let p = unsafe {
                dr_doc_attach_raw_source(
                    &d,
                    0,
                    ORIGINAL.as_ptr(),
                    ORIGINAL.len(),
                    raw.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(!p.is_null(), "{}", text(err));
            Self::Byte(*unsafe { Box::from_raw(p) })
        };
        assert!(err.is_null());
        out
    }
    fn attach(&self, masks: &[&RawLocalMask]) -> Result<Self, String> {
        let description = c(descriptor(masks));
        let pointers = masks
            .iter()
            .map(|m| *m as *const RawLocalMask)
            .collect::<Vec<_>>();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                Self::Byte(d) => {
                    let p = dr_doc_attach_raw_local_masks(
                        d,
                        0,
                        description.as_ptr(),
                        pointers.as_ptr(),
                        pointers.len(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    );
                    (!p.is_null()).then(|| Self::Byte(*Box::from_raw(p)))
                }
                Self::Typed(d) => {
                    let p = dr_precision_doc_attach_raw_local_masks(
                        d,
                        0,
                        description.as_ptr(),
                        pointers.as_ptr(),
                        pointers.len(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    );
                    (!p.is_null()).then(|| Self::Typed(*Box::from_raw(p)))
                }
            }
        };
        match out {
            Some(doc) if err.is_null() => Ok(doc),
            _ => Err(text(err)),
        }
    }
    fn restored(&self, index: usize) -> Vec<u8> {
        let mut err = ptr::null_mut();
        let p = unsafe {
            match self {
                Self::Byte(d) => dr_doc_raw_local_mask(d, 0, index, &mut err),
                Self::Typed(d) => dr_precision_doc_raw_local_mask(d, 0, index, &mut err),
            }
        };
        assert!(!p.is_null(), "{}", text(err));
        let mask = unsafe { Box::from_raw(p) };
        mask.encoded().to_vec()
    }
    fn save(&self, folder: &TempDir, name: &str) -> Vec<u8> {
        let path = folder.path().join(name);
        let path_c = CString::new(path.to_str().unwrap()).unwrap();
        let mut err = ptr::null_mut();
        let ok = unsafe {
            match self {
                Self::Byte(d) => dr_doc_save_native(d, path_c.as_ptr(), std::ptr::null(), &mut err),
                Self::Typed(d) => dr_precision_doc_save_native(
                    d,
                    path_c.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                ),
            }
        };
        assert!(ok, "{}", text(err));
        std::fs::read(path).unwrap()
    }
    fn open(bytes: &[u8], typed: bool, folder: &TempDir) -> Result<Self, String> {
        if typed {
            return PrecisionDocument::parse_native(bytes, LIMIT, 0, None).map(Self::Typed);
        }
        let path = folder.path().join("read.rz");
        std::fs::write(&path, bytes).unwrap();
        let path = CString::new(path.to_str().unwrap()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe { dr_doc_open(path.as_ptr(), std::ptr::null(), &mut err) };
        if out.is_null() {
            Err(text(err))
        } else {
            Ok(Self::Byte(unsafe { *Box::from_raw(out) }))
        }
    }
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn seventeen_masks_refuse_to_attach_at_both_depths() {
    let all = masks(17);
    let refs: Vec<&RawLocalMask> = all.iter().collect();
    for typed in [false, true] {
        let error = Doc::fresh(typed)
            .attach(&refs)
            .err()
            .expect("17 masks refused");
        assert!(error.contains("zero to sixteen"), "{error}");
    }
}

#[test]
fn sixteen_masks_round_trip_with_word_79_and_four_write_none() {
    let folder = TempDir::new().unwrap();
    let all = masks(16);
    let refs: Vec<&RawLocalMask> = all.iter().collect();
    for typed in [false, true] {
        let sixteen = Doc::fresh(typed).attach(&refs).unwrap();
        let bytes = sixteen.save(&folder, "sixteen.rz");
        assert_eq!(&bytes[..4], b"DRDC");
        assert_eq!(
            word(&bytes, 8),
            79,
            "typed {typed}: word 79 after the identity word"
        );
        let reopened = Doc::open(&bytes, typed, &folder).unwrap();
        for (i, mask) in refs.iter().enumerate() {
            assert_eq!(
                reopened.restored(i),
                mask.encoded(),
                "typed {typed} mask {i}"
            );
        }
        assert_eq!(
            reopened.save(&folder, "again.rz"),
            bytes,
            "typed {typed}: reopen and resave is byte-identical"
        );
        // The word removed: the sources still claim it.
        let mut bare = bytes[..8].to_vec();
        bare.extend_from_slice(&bytes[12..]);
        let error = Doc::open(&bare, typed, &folder)
            .err()
            .expect("missing word");
        assert!(error.contains(MISSING_WORD), "typed {typed}: {error}");

        let four = Doc::fresh(typed).attach(&refs[..4]).unwrap();
        let bytes = four.save(&folder, "four.rz");
        assert_ne!(
            word(&bytes, 8),
            79,
            "typed {typed}: four masks write no word"
        );
        assert!(Doc::open(&bytes, typed, &folder).is_ok());
        // The word added: no source needs it.
        let mut claimed = bytes[..8].to_vec();
        claimed.extend_from_slice(&79_u32.to_le_bytes());
        claimed.extend_from_slice(&bytes[8..]);
        let error = Doc::open(&claimed, typed, &folder)
            .err()
            .expect("unclaimed word");
        assert!(
            error.contains("invalid native RAW locals word"),
            "typed {typed}: {error}"
        );
    }
}
