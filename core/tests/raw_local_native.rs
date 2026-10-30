//! Independent native sharing, admission, forward-version and refusal checks.
//! Pixel/field/profile numerical oracles live in raw_local_numeric.rs.
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free};
use doctored_core::ffi_doc::{dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_raw_local::*;
use doctored_core::ffi_raw_source::{dr_doc_attach_raw_source, dr_precision_doc_attach_raw_source};
use doctored_core::ffi_resources::dr_mixed_resources_report_with_raw_masks;
use doctored_core::ffi_source_attachment::{
    dr_doc_source_attachment_kind, dr_precision_doc_source_attachment_kind,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::raw_local_mask::RawLocalMask;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::{ptr, sync::Arc};
use tempfile::TempDir;

use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const ORIGINAL: &[u8] = b"opaque original camera bytes for independent native test";
fn c(v: Value) -> CString {
    CString::new(v.to_string()).unwrap()
}
unsafe fn text(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
    unsafe { dr_string_free(p) };
    s
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
fn mask() -> Box<RawLocalMask> {
    let cmd = c(json!({"op":"linear","from":[0,0],"to":[3,2]}));
    let mut err = ptr::null_mut();
    let p = unsafe {
        dr_raw_local_mask_edit(
            ptr::null(),
            3,
            2,
            cmd.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", unsafe { text(err) });
    assert!(!p.is_null());
    unsafe { Box::from_raw(p) }
}

#[test]
fn every_mask_command_refuses_unknown_fields_without_changing_prior_coverage() {
    let prior = mask();
    let saved = prior.encoded().to_vec();
    let commands = [
        json!({"op":"empty","unknown":1}),
        json!({"op":"full","unknown":1}),
        json!({"op":"invert","unknown":1}),
        json!({"op":"radial","center":[1,1],"radius":[1,1],"hardness":0,"unknown":1}),
        json!({"op":"linear","from":[0,0],"to":[3,2],"unknown":1}),
        json!({"op":"brush","mode":"add","dabs":[],"unknown":1}),
        json!({"op":"brush","mode":"add","dabs":[{"center":[1,1],"radius":1,"hardness":0,"opacity":1,"unknown":1}]}),
    ];
    for command in commands {
        let command = c(command);
        let mut error = ptr::null_mut();
        let result = unsafe {
            dr_raw_local_mask_edit(
                &*prior,
                3,
                2,
                command.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(result.is_null(), "accepted {}", command.to_str().unwrap());
        let message = unsafe { text(error) };
        assert!(message.contains("unknown field"), "{message}");
        assert_eq!(prior.encoded(), saved);
    }
    // Exact schema forms still execute; refusal must not reject the command.
    for command in [json!({"op":"empty"}), json!({"op":"invert"})] {
        let command = c(command);
        let mut error = ptr::null_mut();
        let result = unsafe {
            dr_raw_local_mask_edit(
                &*prior,
                3,
                2,
                command.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(error.is_null(), "{}", unsafe { text(error) });
        assert!(!result.is_null());
        unsafe { dr_raw_local_mask_free(result) };
    }
}
fn descriptor(masks: &[&RawLocalMask]) -> Value {
    json!({"schema_version":1,"kind":"raw_local_masks","state":{"schema_version":1,"coordinates":"oriented-source-pixel-centers-v1","width":3,"height":2,"nodes":(0..masks.len()).map(|i|format!("node{i}")).collect::<Vec<_>>()},
 "resources":masks.iter().enumerate().map(|(i,m)|json!({"id":format!("node{i}"),"media_type":"com.rasterize.raw-local-mask","sha256":"a".repeat(64),"byte_length":m.encoded().len()})).collect::<Vec<_>>()})
}
enum Doc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}
impl Doc {
    fn fresh(typed: bool, revision: &str) -> Self {
        let source = master();
        let request=SourceEdit::parse(&json!({"op":"capture_raster","layer":0,"source_id":"camera","revision_id":revision,"source_transform":[1,0,0,1,0,0],"depth_conversion":"convert_fallback"}).to_string()).unwrap();
        let base = if typed {
            Self::Typed(master())
        } else {
            Self::Byte(DrDocument::from_pixels(RgbaImage::from_pixel(
                3,
                2,
                Rgba([0, 0, 0, 0]),
            )))
        };
        let retained = match base {
            Self::Byte(d) => Self::Byte(
                d.source_edit_input(Some(SourceInput::Typed(&source)), &request, LIMIT, 0, None)
                    .unwrap()
                    .unwrap(),
            ),
            Self::Typed(d) => Self::Typed(
                d.source_edit_input(Some(SourceInput::Typed(&source)), &request, LIMIT, 0, None)
                    .unwrap()
                    .unwrap(),
            ),
        };
        let raw = c(
            json!({"schema_version":1,"original_filename":"fixture.dng","content_sha256":"b".repeat(64),"decoder":{"identifier":"independent","version":"1","implementation":"core-only-fixture"},"recipe":{"schema_version":3,"tint":11.999950408935547_f64}}),
        );
        let mut err = ptr::null_mut();
        let out = unsafe {
            match retained {
                Self::Byte(d) => {
                    let p = dr_doc_attach_raw_source(
                        &d,
                        0,
                        ORIGINAL.as_ptr(),
                        ORIGINAL.len(),
                        raw.as_ptr(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    );
                    assert!(!p.is_null(), "{}", text(err));
                    Self::Byte(*Box::from_raw(p))
                }
                Self::Typed(d) => {
                    let p = dr_precision_doc_attach_raw_source(
                        &d,
                        0,
                        ORIGINAL.as_ptr(),
                        ORIGINAL.len(),
                        raw.as_ptr(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    );
                    assert!(!p.is_null(), "{}", text(err));
                    Self::Typed(*Box::from_raw(p))
                }
            }
        };
        assert!(err.is_null());
        out
    }
    fn attach(
        &self,
        description: Option<Value>,
        masks: &[&RawLocalMask],
        limit: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let description = description.map(c);
        let masks = masks
            .iter()
            .map(|m| *m as *const RawLocalMask)
            .collect::<Vec<_>>();
        let mut err = ptr::null_mut();
        let result = unsafe {
            match self {
                Self::Byte(d) => {
                    let p = dr_doc_attach_raw_local_masks(
                        d,
                        0,
                        description.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
                        masks.as_ptr(),
                        masks.len(),
                        limit,
                        0,
                        control.map_or(ptr::null(), |c| c),
                        &mut err,
                    );
                    if p.is_null() {
                        None
                    } else {
                        Some(Self::Byte(*Box::from_raw(p)))
                    }
                }
                Self::Typed(d) => {
                    let p = dr_precision_doc_attach_raw_local_masks(
                        d,
                        0,
                        description.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
                        masks.as_ptr(),
                        masks.len(),
                        limit,
                        0,
                        control.map_or(ptr::null(), |c| c),
                        &mut err,
                    );
                    if p.is_null() {
                        None
                    } else {
                        Some(Self::Typed(*Box::from_raw(p)))
                    }
                }
            }
        };
        if err.is_null() {
            Ok(result)
        } else {
            Err(unsafe { text(err) })
        }
    }
    fn restored(&self, index: usize) -> Result<Box<RawLocalMask>, String> {
        let mut err = ptr::null_mut();
        let p = unsafe {
            match self {
                Self::Byte(d) => dr_doc_raw_local_mask(d, 0, index, &mut err),
                Self::Typed(d) => dr_precision_doc_raw_local_mask(d, 0, index, &mut err),
            }
        };
        if p.is_null() {
            Err(unsafe { text(err) })
        } else {
            assert!(err.is_null());
            Ok(unsafe { Box::from_raw(p) })
        }
    }
    fn kind(&self) -> u32 {
        unsafe {
            match self {
                Self::Byte(d) => dr_doc_source_attachment_kind(d, 0),
                Self::Typed(d) => dr_precision_doc_source_attachment_kind(d, 0),
            }
        }
    }
    fn save(&self, path: &std::path::Path) -> Vec<u8> {
        let path_c = CString::new(path.to_str().unwrap()).unwrap();
        let mut err = ptr::null_mut();
        let ok = unsafe {
            match self {
                Self::Byte(d) => dr_doc_save_native(d, path_c.as_ptr(), &mut err),
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
        assert!(ok, "{}", unsafe { text(err) });
        assert!(err.is_null());
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
        let out = unsafe { dr_doc_open(path.as_ptr(), &mut err) };
        if out.is_null() {
            Err(unsafe { text(err) })
        } else {
            assert!(err.is_null());
            Ok(Self::Byte(unsafe { *Box::from_raw(out) }))
        }
    }
    fn mask_union(&self, masks: &[&RawLocalMask]) -> Value {
        let (byte, typed) = match self {
            Self::Byte(d) => (d as *const DrDocument, ptr::null()),
            Self::Typed(d) => (ptr::null(), d as *const PrecisionDocument),
        };
        let masks = masks
            .iter()
            .map(|m| *m as *const RawLocalMask)
            .collect::<Vec<_>>();
        let mut err = ptr::null_mut();
        let p = unsafe {
            dr_mixed_resources_report_with_raw_masks(
                &byte,
                1,
                ptr::null(),
                0,
                &typed,
                1,
                ptr::null(),
                0,
                masks.as_ptr(),
                masks.len(),
                0,
                LIMIT,
                &mut err,
            )
        };
        assert!(err.is_null());
        serde_json::from_str(&unsafe { text(p) }).unwrap()
    }
}
fn tag(data: &[u8], needle: &[u8]) -> usize {
    data.windows(needle.len())
        .position(|v| v == needle)
        .unwrap()
}
fn poisoned(mut data: Vec<u8>) -> Vec<u8> {
    let at = tag(&data, b"IDAT");
    data[at + 4] ^= 255;
    data
}
fn rewrite_state(bytes: &[u8], f: impl FnOnce(&mut Value)) -> Vec<u8> {
    let at = tag(bytes, b"ATCH");
    let raw = at + 8;
    assert_eq!(bytes[raw], 1);
    let raw_len = u32::from_le_bytes(bytes[raw + 5..raw + 9].try_into().unwrap()) as usize;
    let attachment = raw + 9 + raw_len;
    assert_eq!(bytes[attachment], 1);
    let at = attachment + 1;
    let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut v: Value = serde_json::from_slice(&bytes[at + 4..at + 4 + len]).unwrap();
    f(&mut v);
    let value = serde_json::from_value::<doctored_core::source_attachment::AttachmentDescriptor>(v)
        .unwrap()
        .to_json()
        .unwrap();
    let mut out = bytes[..at].to_vec();
    out.extend((value.len() as u32).to_le_bytes());
    out.extend(value.as_bytes());
    out.extend(&bytes[at + 4 + len..]);
    out
}
#[test]
fn mask_handles_and_both_native_depths_share_one_blob_and_roundtrip_exactly() {
    for typed in [false, true] {
        let m = mask();
        let base = Doc::fresh(typed, "v1");
        assert_eq!(base.kind(), 0);
        let next = base
            .attach(Some(descriptor(&[&m, &m])), &[&m, &m], LIMIT, None)
            .unwrap()
            .unwrap();
        assert_eq!(next.kind(), 2);
        let a = next.restored(0).unwrap();
        let b = next.restored(1).unwrap();
        assert_eq!(a.encoded().as_ptr(), m.encoded().as_ptr());
        assert_eq!(b.encoded().as_ptr(), m.encoded().as_ptr());
        assert_eq!(
            next.mask_union(&[])["unique_retained_bytes"],
            next.mask_union(&[&m, &a, &b])["unique_retained_bytes"]
        );
        let dir = TempDir::new().unwrap();
        let bytes = next.save(&dir.path().join("initial.rz"));
        // Offset 8: 12E-1's identity wrapper owns the outermost word.
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            if typed { 46 } else { 45 }
        );
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            if typed { 22 } else { 21 }
        );
        let pool = tag(&bytes, b"RBIN");
        assert_eq!(
            u32::from_le_bytes(bytes[pool + 4..pool + 8].try_into().unwrap()),
            2
        ); // RAW plus one shared mask.
        let mut restored = Doc::open(&bytes, typed, &dir).unwrap();
        for cycle in 0..3 {
            let a = restored.restored(0).unwrap();
            let b = restored.restored(1).unwrap();
            assert_eq!(a.encoded(), m.encoded());
            assert_eq!(a.encoded().as_ptr(), b.encoded().as_ptr());
            let bytes = restored.save(&dir.path().join(format!("cycle{cycle}.rz")));
            assert!(bytes.windows(ORIGINAL.len()).any(|s| s == ORIGINAL));
            restored = Doc::open(&bytes, typed, &dir).unwrap();
        }
    }
}
/// The mask resource's media type after the Rasterize → Doctored rename and
/// the one written before it both attach, save and reopen with the same
/// mask at both depths; any other media type is refused.
#[test]
fn both_mask_media_types_open_and_another_is_refused() {
    for typed in [false, true] {
        let dir = TempDir::new().unwrap();
        let m = mask();
        for media in [
            "com.kgalligan.doctored.raw-local-mask",
            "com.rasterize.raw-local-mask",
        ] {
            let mut description = descriptor(&[&m]);
            description["resources"][0]["media_type"] = json!(media);
            let doc = Doc::fresh(typed, "v1")
                .attach(Some(description), &[&m], LIMIT, None)
                .unwrap()
                .unwrap();
            let bytes = doc.save(&dir.path().join("media.rz"));
            let reopened = Doc::open(&bytes, typed, &dir)
                .unwrap_or_else(|e| panic!("{media}, typed {typed}: {e}"));
            assert_eq!(reopened.restored(0).unwrap().encoded(), m.encoded());
        }
        let mut description = descriptor(&[&m]);
        description["resources"][0]["media_type"] = json!("com.example.raw-local-mask");
        let refused = match Doc::fresh(typed, "v1").attach(Some(description), &[&m], LIMIT, None) {
            Ok(Some(doc)) => {
                let bytes = doc.save(&dir.path().join("other.rz"));
                Doc::open(&bytes, typed, &dir).is_err()
            }
            Ok(None) | Err(_) => true,
        };
        assert!(refused, "typed {typed}: an unknown media type is refused");
    }
}
#[test]
fn malformed_known_mask_and_state_records_refuse_before_any_png_decode() {
    for typed in [false, true] {
        let dir = TempDir::new().unwrap();
        let m = mask();
        let doc = Doc::fresh(typed, "v1")
            .attach(Some(descriptor(&[&m])), &[&m], LIMIT, None)
            .unwrap()
            .unwrap();
        let bytes = doc.save(&dir.path().join("valid.rz"));
        let pos = tag(&bytes, b"RZLM");
        let mut cases = Vec::new();
        let mut bad = bytes.clone();
        bad[pos] = b'X';
        cases.push(bad);
        let mut bad = bytes.clone();
        bad[pos + 8..pos + 12].copy_from_slice(&4u32.to_le_bytes());
        cases.push(bad);
        for mutation in ["width", "nodes", "coordinates", "version_type"] {
            cases.push(rewrite_state(&bytes, |v| match mutation {
                "width" => v["state"]["width"] = json!(4),
                "nodes" => v["state"]["nodes"] = json!(["not-the-resource"]),
                "coordinates" => v["state"]["coordinates"] = json!("wrong-space"),
                _ => v["state"]["schema_version"] = json!(true),
            }));
        }
        for bad in cases {
            let err = match Doc::open(&poisoned(bad), typed, &dir) {
                Ok(_) => panic!("malformed local record accepted"),
                Err(e) => e,
            };
            assert!(
                err.contains("local mask") || err.contains("local-mask"),
                "{err}"
            );
        }
    }
}
#[test]
fn future_binary_and_state_versions_keep_cached_native_but_cannot_materialize() {
    for typed in [false, true] {
        let dir = TempDir::new().unwrap();
        let m = mask();
        let doc = Doc::fresh(typed, "v1")
            .attach(Some(descriptor(&[&m])), &[&m], LIMIT, None)
            .unwrap()
            .unwrap();
        let bytes = doc.save(&dir.path().join("valid.rz"));
        let pos = tag(&bytes, b"RZLM");
        let mut future = bytes.clone();
        future[pos + 4..pos + 8].copy_from_slice(&93u32.to_le_bytes());
        for input in [
            future,
            rewrite_state(&bytes, |v| {
                v["state"] = json!({"schema_version":94,"future_policy":"opaque"})
            }),
        ] {
            let reopened = Doc::open(&input, typed, &dir).unwrap();
            assert_eq!(reopened.kind(), 2);
            assert!(reopened.restored(0).is_err());
            let saved = reopened.save(&dir.path().join("future.rz"));
            assert_eq!(saved, input);
        }
        let mut malformed_future =
            rewrite_state(&bytes, |v| v["state"]["schema_version"] = json!(94));
        let at = tag(&malformed_future, b"RZLM");
        malformed_future[at + 12..at + 16].copy_from_slice(&9u32.to_le_bytes());
        assert!(Doc::open(&poisoned(malformed_future), typed, &dir).is_err());
    }
}
#[test]
fn private_attachment_publication_is_atomic_admitted_and_never_clears_unknown_state() {
    let m = mask();
    for typed in [false, true] {
        let doc = Doc::fresh(typed, "v1");
        assert!(doc.attach(None, &[], LIMIT, None).unwrap().is_none());
        assert!(doc.attach(Some(descriptor(&[&m])), &[&m], 1, None).is_err());
        assert_eq!(doc.kind(), 0);
        let control = DrJobControl::default();
        control.cancel();
        assert!(doc
            .attach(Some(descriptor(&[&m])), &[&m], LIMIT, Some(&control))
            .is_err());
        assert_eq!(control.scratch_bytes(), 0);
        let mut invalid = descriptor(&[&m]);
        invalid["state"]["schema_version"] = json!(2);
        assert!(doc.attach(Some(invalid), &[&m], LIMIT, None).is_err());
        let attached = doc
            .attach(Some(descriptor(&[&m])), &[&m], LIMIT, None)
            .unwrap()
            .unwrap();
        assert!(attached.attach(None, &[], LIMIT, None).is_err());
        assert!(attached
            .attach(Some(descriptor(&[&m])), &[&m], LIMIT, None)
            .is_err());
        assert_eq!(attached.kind(), 2);
    }
}
#[test]
fn mask_copy_admission_cancellation_caps_and_single_use_leave_output_untouched() {
    let m = mask();
    let mut out = vec![0xcd; m.encoded().len()];
    let mut err = ptr::null_mut();
    assert!(!unsafe {
        dr_raw_local_mask_copy(
            &*m,
            out.as_mut_ptr(),
            out.len(),
            1,
            0,
            ptr::null(),
            &mut err,
        )
    });
    assert!(!err.is_null());
    unsafe { text(err) };
    assert!(out.iter().all(|v| *v == 0xcd));
    let control = DrJobControl::default();
    control.cancel();
    err = ptr::null_mut();
    assert!(!unsafe {
        dr_raw_local_mask_copy(
            &*m,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            &control,
            &mut err,
        )
    });
    unsafe { text(err) };
    assert!(out.iter().all(|v| *v == 0xcd));
    assert_eq!(control.scratch_bytes(), 0);
    let control = DrJobControl::default();
    err = ptr::null_mut();
    assert!(unsafe {
        dr_raw_local_mask_copy(
            &*m,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            &control,
            &mut err,
        )
    });
    assert_eq!(out, m.encoded());
    assert!(err.is_null());
    assert!(!unsafe {
        dr_raw_local_mask_copy(
            &*m,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            &control,
            &mut err,
        )
    });
    unsafe { text(err) };
    let bad = c(json!({"op":"radial","center":[0,0],"radius":[1,1],"hardness":0}));
    err = ptr::null_mut();
    assert!(unsafe {
        dr_raw_local_mask_edit(
            ptr::null(),
            64_000_001,
            1,
            bad.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert!(!err.is_null());
    unsafe { text(err) };
    let short = [b'R', b'Z', b'L', b'M'];
    err = ptr::null_mut();
    assert!(unsafe {
        dr_raw_local_mask_from_bytes(short.as_ptr(), short.len(), LIMIT, 0, ptr::null(), &mut err)
    }
    .is_null());
    unsafe { text(err) };
}
/// `dr_raw_local_mask_from_coverage` over `samples`: the mask, NULL-without-error, or the error.
fn coverage(
    samples: Option<&[u8]>,
    length: usize,
    width: u32,
    height: u32,
    limit: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<Box<RawLocalMask>>, String> {
    let mut err = ptr::null_mut();
    let p = unsafe {
        dr_raw_local_mask_from_coverage(
            samples.map_or(ptr::null(), |s| s.as_ptr()),
            length,
            width,
            height,
            limit,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut err,
        )
    };
    if !err.is_null() {
        assert!(p.is_null());
        return Err(unsafe { text(err) });
    }
    Ok((!p.is_null()).then(|| unsafe { Box::from_raw(p) }))
}
#[test]
fn byte_coverage_is_stored_as_the_same_fraction_in_u16_and_bad_planes_are_refused() {
    // Every byte code once, in an order that is not the identity, on a 32×8 plane.
    let samples: Vec<u8> = (0..256u32).map(|i| ((i * 7 + 3) % 256) as u8).collect();
    let mut seen = samples.clone();
    seen.sort_unstable();
    assert!(seen.iter().enumerate().all(|(i, v)| usize::from(*v) == i));
    let mask = coverage(Some(&samples), 256, 32, 8, LIMIT, None)
        .unwrap()
        .unwrap();
    let encoded = mask.encoded();
    assert_eq!(encoded.len(), 16 + 256 * 2);
    assert_eq!(&encoded[..4], b"RZLM");
    let word = |at: usize| u32::from_le_bytes(encoded[at..at + 4].try_into().unwrap());
    assert_eq!((word(4), word(8), word(12)), (1, 32, 8));
    assert_eq!(mask.dimensions(), (32, 8));
    for (index, byte) in samples.iter().enumerate() {
        // The oracle is the fraction, not the multiplier: round(v / 255 · 65535).
        let expected = (f64::from(*byte) / 255.0 * 65535.0).round() as u16;
        let at = 16 + index * 2;
        assert_eq!(
            u16::from_le_bytes([encoded[at], encoded[at + 1]]),
            expected,
            "sample {index} (byte {byte})"
        );
        assert_eq!(mask.sample(index), Some(expected));
    }
    // A plane with no coverage is not a mask: NULL and no error.
    let zero = vec![0u8; 256];
    assert!(coverage(Some(&zero), 256, 32, 8, LIMIT, None)
        .unwrap()
        .is_none());
    // One covered sample anywhere is.
    let mut one = zero.clone();
    one[255] = 1;
    let single = coverage(Some(&one), 256, 32, 8, LIMIT, None)
        .unwrap()
        .unwrap();
    assert_eq!(single.sample(255), Some(257));
    assert_eq!(single.sample(254), Some(0));
    for (what, result) in [
        (
            "short length",
            coverage(Some(&samples), 255, 32, 8, LIMIT, None),
        ),
        (
            "long length",
            coverage(Some(&samples), 257, 32, 8, LIMIT, None),
        ),
        ("zero width", coverage(Some(&samples), 0, 0, 8, LIMIT, None)),
        (
            "zero height",
            coverage(Some(&samples), 0, 32, 0, LIMIT, None),
        ),
        (
            "over 64 megapixels",
            coverage(Some(&samples), 64_000_001, 64_000_001, 1, LIMIT, None),
        ),
        ("null samples", coverage(None, 256, 32, 8, LIMIT, None)),
        (
            "a limit below the plane",
            coverage(Some(&samples), 256, 32, 8, 1, None),
        ),
    ] {
        match result {
            Err(message) => assert!(!message.is_empty(), "{what}"),
            Ok(_) => panic!("{what} was accepted"),
        }
    }
    let control = DrJobControl::default();
    control.cancel();
    assert!(coverage(Some(&samples), 256, 32, 8, LIMIT, Some(&control)).is_err());
    assert_eq!(control.scratch_bytes(), 0);
    // A control is single-use here as everywhere on this surface.
    let control = DrJobControl::default();
    assert!(coverage(Some(&samples), 256, 32, 8, LIMIT, Some(&control))
        .unwrap()
        .is_some());
    assert_eq!(control.scratch_bytes(), 0);
    assert!(coverage(Some(&samples), 256, 32, 8, LIMIT, Some(&control)).is_err());
}
#[test]
fn a_generated_coverage_mask_survives_a_native_save_and_reopen_bit_for_bit_at_both_depths() {
    // 3×2, the size of the fixture's developed source.
    let samples = [0u8, 1, 127, 128, 254, 255];
    for typed in [false, true] {
        let generated = coverage(Some(&samples), 6, 3, 2, LIMIT, None)
            .unwrap()
            .unwrap();
        let drawn = mask();
        let saved = generated.encoded().to_vec();
        assert_eq!(
            saved[16..],
            [0, 0, 1, 1, 127, 127, 128, 128, 254, 254, 255, 255]
        );
        let doc = Doc::fresh(typed, "v1")
            .attach(
                Some(descriptor(&[&generated, &drawn])),
                &[&generated, &drawn],
                LIMIT,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(doc.kind(), 2);
        // The attachment shares the generated blob like any other mask.
        assert_eq!(
            doc.restored(0).unwrap().encoded().as_ptr(),
            generated.encoded().as_ptr()
        );
        let dir = TempDir::new().unwrap();
        let mut bytes = doc.save(&dir.path().join("generated.rz"));
        for cycle in 0..2 {
            let reopened = Doc::open(&bytes, typed, &dir).unwrap();
            assert_eq!(reopened.restored(0).unwrap().encoded(), saved);
            assert_eq!(reopened.restored(1).unwrap().encoded(), drawn.encoded());
            bytes = reopened.save(&dir.path().join(format!("generated{cycle}.rz")));
        }
    }
}
#[test]
fn live_brush_cancellation_polls_work_and_releases_all_scratch() {
    let command = c(
        json!({"op":"brush","mode":"add","dabs":(0..40).map(|_|json!({"center":[1000,1000],"radius":1000,"hardness":0.1,"opacity":0.02})).collect::<Vec<_>>()}),
    );
    let render = |token: usize| {
        let mut err = ptr::null_mut();
        let p = unsafe {
            dr_raw_local_mask_edit(
                ptr::null(),
                2000,
                2000,
                command.as_ptr(),
                LIMIT,
                0,
                token as *const DrJobControl,
                &mut err,
            )
        };
        let accepted = !p.is_null();
        if !p.is_null() {
            unsafe { dr_raw_local_mask_free(p) }
        };
        (accepted, unsafe { text(err) })
    };
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress > 0.0, render);
    let n0 = entry.checkpoints;
    let k = 1;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 1 + k,
        "the entry's kernel polls fewer than {} checkpoints before its next report (entry {n0}, next {next:?})",
        k + 1
    );
    let parked = cancelled_parked_at(
        n0 + 1 + k,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    println!("live brush cancellation: {}", parked_line(&parked));
    assert!(parked.scratch > 0);
    assert!(parked.progress > 0.0 && parked.progress < 1.0);
}
#[test]
fn all_new_exports_tolerate_null_handles_and_empty_arrays() {
    unsafe {
        let mut err: *mut c_char;
        dr_raw_local_mask_free(ptr::null_mut());
        assert_eq!(dr_doc_source_attachment_kind(ptr::null(), 0), 0);
        assert_eq!(dr_precision_doc_source_attachment_kind(ptr::null(), 0), 0);
        macro_rules! refused {
            ($call:expr) => {{
                err = ptr::null_mut();
                assert!(($call).is_null());
                assert!(!err.is_null());
                text(err);
            }};
        }
        refused!(dr_raw_local_mask_info(ptr::null(), &mut err));
        let mut n = 23;
        refused!(dr_raw_local_mask_bytes(ptr::null(), &mut n, &mut err));
        assert_eq!(n, 0);
        refused!(dr_raw_local_mask_edit(
            ptr::null(),
            0,
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        refused!(dr_raw_local_mask_from_bytes(
            ptr::null(),
            16,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        refused!(dr_raw_local_mask_from_coverage(
            ptr::null(),
            6,
            3,
            2,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        err = ptr::null_mut();
        assert!(!dr_raw_local_mask_copy(
            ptr::null(),
            ptr::null_mut(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        text(err);
        refused!(dr_raw_local_mask_preview(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            0,
            1,
            1,
            1,
            1.0,
            0.0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        let mut lab = [7.0_f64; 3];
        err = ptr::null_mut();
        assert!(!dr_precision_doc_raw_range_sample(
            ptr::null(),
            0.0,
            0.0,
            lab.as_mut_ptr(),
            &mut err
        ));
        assert!(!err.is_null());
        text(err);
        assert_eq!(lab, [7.0; 3]);
        let sampled = master();
        err = ptr::null_mut();
        assert!(!dr_precision_doc_raw_range_sample(
            &sampled,
            0.0,
            0.0,
            ptr::null_mut(),
            &mut err
        ));
        assert!(!err.is_null());
        text(err);
        refused!(dr_precision_doc_develop_locals(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        refused!(dr_precision_doc_from_image(
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        refused!(dr_doc_raw_local_mask(ptr::null(), 0, 0, &mut err));
        refused!(dr_precision_doc_raw_local_mask(ptr::null(), 0, 0, &mut err));
        refused!(dr_doc_attach_raw_local_masks(
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        refused!(dr_precision_doc_attach_raw_local_masks(
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        err = ptr::null_mut();
        let p = dr_mixed_resources_report_with_raw_masks(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            LIMIT,
            &mut err,
        );
        assert!(err.is_null());
        let v: Value = serde_json::from_str(&text(p)).unwrap();
        assert_eq!(v["raw_local_mask_handle_count"], 0);
    }
}

#[test]
fn mask_info_display_region_and_plain_source_extraction_preserve_their_contracts() {
    let mask = mask();
    let mut error = ptr::null_mut();
    let info: Value =
        serde_json::from_str(&unsafe { text(dr_raw_local_mask_info(&*mask, &mut error)) }).unwrap();
    assert!(error.is_null());
    assert_eq!(info["width"], 3);
    assert_eq!(info["height"], 2);
    assert_eq!(info["byte_length"], 28);
    assert_eq!(info["storage"], "u16_le");
    let image = unsafe {
        dr_raw_local_mask_preview(
            &*mask,
            ptr::null(),
            ptr::null(),
            1,
            0,
            2,
            2,
            1,
            0.5,
            0.0,
            true,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!image.is_null());
    assert!(error.is_null());
    let image = unsafe { Box::from_raw(image) };
    assert_eq!(
        unsafe { (dr_image_width(&*image), dr_image_height(&*image)) },
        (2, 2)
    );
    let display = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(&*image), 16) };
    for y in 0..2 {
        for x in 0..2 {
            let alpha = (f64::from(mask.sample(y * 3 + x + 1).unwrap()) / 65535.0 * 0.5 * 255.0)
                .round() as u8;
            assert_eq!(
                display[(y * 2 + x) * 4..(y * 2 + x) * 4 + 4],
                [255, 0, 0, alpha]
            );
        }
    }
    let base = master();
    let source = base.layer_source(0).unwrap();
    let document =
        unsafe { dr_precision_doc_from_image(source, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!document.is_null());
    assert!(error.is_null());
    let document = unsafe { Box::from_raw(document) };
    assert!(PrecisionImage::ptr_eq(
        source,
        document.layer_source(0).unwrap()
    ));
    assert!(Arc::ptr_eq(source.profile(), document.profile()));
    let denied = unsafe { dr_precision_doc_from_image(source, 1, 0, ptr::null(), &mut error) };
    assert!(denied.is_null());
    assert!(!error.is_null());
    unsafe { text(error) };
}

#[test]
fn local_development_rejects_budget_mismatched_inputs_and_precancel_without_mutation() {
    let base = master();
    let mask = mask();
    let variants = [&base as *const PrecisionDocument];
    let masks = [&*mask as *const RawLocalMask];
    let settings = c(json!([{"enabled":true,"density":0.75,"feather":1.0}]));
    let original = base.layer_source(0).unwrap().to_rgba16().unwrap();
    for (count, limit, control) in [
        (1, 1, None),
        (2, LIMIT, None),
        (
            1,
            LIMIT,
            Some({
                let c = DrJobControl::default();
                c.cancel();
                c
            }),
        ),
    ] {
        let mut error = ptr::null_mut();
        // Count mismatch uses valid arrays with two entries, avoiding a caller
        // memory-contract violation while checking the settings-count refusal.
        let v = [variants[0], variants[0]];
        let m = [masks[0], masks[0]];
        let result = unsafe {
            dr_precision_doc_develop_locals(
                &base,
                v.as_ptr(),
                m.as_ptr(),
                count,
                settings.as_ptr(),
                ptr::null(),
                limit,
                0,
                control.as_ref().map_or(ptr::null(), |c| c),
                &mut error,
            )
        };
        assert!(result.is_null());
        assert!(!error.is_null());
        unsafe { text(error) };
        if let Some(c) = control {
            assert_eq!(c.scratch_bytes(), 0);
        }
        assert_eq!(
            base.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
            original
        );
    }
}

#[test]
fn full_camera_feather_preview_uses_queried_scratch_instead_of_a_fixed_display_allowance() {
    use doctored_core::ffi_jobs::dr_job_control_set_scratch_limit;
    let empty = c(json!({"op":"empty"}));
    let mut error = ptr::null_mut();
    let mask = unsafe {
        dr_raw_local_mask_edit(
            ptr::null(),
            3008,
            2000,
            empty.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!mask.is_null());
    assert!(error.is_null());
    let mask = unsafe { Box::from_raw(mask) };
    let plan = unsafe {
        dr_raw_local_mask_preview_scratch(&*mask, 0, 0, 3008, 2000, 4, 0.75, 1.0, true, &mut error)
    };
    assert!(error.is_null());
    assert!(plan > 64 * 1024 * 1024 && plan < 512 * 1024 * 1024);
    let small = DrJobControl::default();
    assert!(unsafe { dr_job_control_set_scratch_limit(&small, 64 * 1024 * 1024) });
    let refused = unsafe {
        dr_raw_local_mask_preview(
            &*mask,
            ptr::null(),
            ptr::null(),
            0,
            0,
            3008,
            2000,
            4,
            0.75,
            1.0,
            true,
            LIMIT,
            0,
            &small,
            &mut error,
        )
    };
    assert!(refused.is_null());
    assert!(!error.is_null());
    unsafe { text(error) };
    assert_eq!(small.scratch_bytes(), 0);
    let control = DrJobControl::default();
    assert!(unsafe { dr_job_control_set_scratch_limit(&control, plan) });
    error = ptr::null_mut();
    let preview = unsafe {
        dr_raw_local_mask_preview(
            &*mask,
            ptr::null(),
            ptr::null(),
            0,
            0,
            3008,
            2000,
            4,
            0.75,
            1.0,
            true,
            LIMIT,
            0,
            &control,
            &mut error,
        )
    };
    assert!(!preview.is_null());
    assert!(error.is_null());
    assert_eq!(control.scratch_bytes(), 0);
    unsafe { doctored_core::ffi::dr_image_free(preview) };
    for (mask, w, divisor, density, feather) in [
        (ptr::null(), 3008, 4, 0.75, 1.0),
        (&*mask as *const _, 3009, 4, 0.75, 1.0),
        (&*mask, 3008, 0, 0.75, 1.0),
        (&*mask, 3008, 4, f64::NAN, 1.0),
        (&*mask, 3008, 4, 0.75, 257.0),
    ] {
        error = ptr::null_mut();
        assert_eq!(
            unsafe {
                dr_raw_local_mask_preview_scratch(
                    mask, 0, 0, w, 2000, divisor, density, feather, true, &mut error,
                )
            },
            0
        );
        assert!(!error.is_null());
        unsafe { text(error) };
    }
}
