//! Canonical composition of independently optional mask29 and blend30/31 wrappers.
use crate::group_roots;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use group_roots::GroupRoots;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

use crate::identity;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
fn edit(doc: &PrecisionDocument, value: Value) -> PrecisionDocument {
    let request = CString::new(value.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        panic!("{message}");
    }
    if result.is_null() {
        doc.clone()
    } else {
        *unsafe { Box::from_raw(result) }
    }
}
fn mode(doc: &PrecisionDocument, index: usize, mode: i32) -> PrecisionDocument {
    edit(
        doc,
        json!({"op":"layer_blend","version":1,"layer":index,"blend_mode":mode}),
    )
}
fn fixture() -> PrecisionDocument {
    let pixels: Vec<_> = (0..12)
        .map(|i| [1001 + i, 2003 + i, 3007 + i, if i == 2 { 0 } else { 32769 }])
        .collect();
    let image = PrecisionImage::from_rgba16(4, 3, pixels, IccProfile::srgb(), None).unwrap();
    let base = Layer::raster("Backdrop", image.clone(), (0, 0), 1., true).unwrap();
    let mut p = MaskProperties::at((1, 1));
    p.linked = false;
    p.density = 0.75;
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(2, 2, vec![0, 123, 32769, 65535], None).unwrap(),
        p,
        true,
    )
    .unwrap();
    let top = Layer::raster("Top", image, (-1, 0), 0.625, true)
        .unwrap()
        .with_mask(Some(mask));
    PrecisionDocument::from_parts(
        4,
        3,
        IccProfile::srgb(),
        Metadata {
            exif: Some(vec![1, 2, 3].into()),
            xmp: Some(b"exact xmp".to_vec().into()),
            iptc: None,
        },
        Resolution {
            x: 301.25,
            y: 299.5,
        },
        vec![base, top],
    )
    .unwrap()
}
fn outside(doc: &PrecisionDocument, index: usize, value: u8) -> PrecisionDocument {
    let mask = doc.layer(index).unwrap().mask().unwrap();
    let mut p = mask.properties().clone();
    p.outside_coverage = value;
    doc.set_layer_mask(
        index,
        Some(mask.with_properties(p, mask.enabled()).unwrap()),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}
/// The encoded file with 12E-1's always-written identity wrapper peeled off.
/// This file's subject is an EARLIER wrapper, so the outer word is asserted
/// and the `LIDS` tail removed, leaving byte-for-byte what the writer these
/// tests were written against produced.
fn bytes(doc: &PrecisionDocument) -> Vec<u8> {
    identity::typed_prior_body(&doc.encode_native(LIMIT, 0, None).unwrap())
}
fn reopen(data: &[u8]) -> PrecisionDocument {
    PrecisionDocument::parse_native(data, LIMIT, 0, None).unwrap()
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn tag(data: &[u8], tag: &[u8]) -> usize {
    data.windows(tag.len()).rposition(|v| v == tag).unwrap()
}
fn mask_wrapper(inner: &[u8], indices: &[u32]) -> Vec<u8> {
    let mut out = inner[..4].to_vec();
    out.extend(words(&[29]));
    out.extend(&inner[4..]);
    out.extend(b"MOUT");
    out.extend(words(&[1, indices.len() as u32]));
    out.extend(words(indices));
    out
}
#[test]
fn both_wrappers_are_independent_exact_canonical_extensions_and_downgrades() {
    let base = fixture();
    let normal = bytes(&base);
    let white = outside(&base, 1, 255);
    assert_eq!(bytes(&white), mask_wrapper(&normal, &[1]));
    for newmode in [
        10, 11, 12, 15, 13, 18, 19, 20, 22, 23, 24, 25, 26, 14, 16, 17, 21,
    ] {
        let version = if [10, 11, 12, 15].contains(&newmode) {
            30
        } else if [13, 18, 19, 20, 22].contains(&newmode) {
            31
        } else if [23, 24, 25, 26].contains(&newmode) {
            33
        } else {
            34
        };
        let plain = mode(&base, 1, newmode);
        let plain_blend = bytes(&plain);
        let combined = mode(&white, 1, newmode);
        let both = bytes(&combined);
        assert_eq!(&both[4..16], words(&[29, version, 13]));
        assert_eq!(both, mask_wrapper(&plain_blend, &[1]));
        assert_eq!(both.len(), plain_blend.len() + 20);
        assert_eq!(
            combined.native_plan().unwrap().encoded_bound,
            plain.native_plan().unwrap().encoded_bound + 20
        );
        let restored = reopen(&both);
        assert_eq!(bytes(&restored), both);
        assert_eq!(bytes(&outside(&restored, 1, 0)), plain_blend);
        assert_eq!(bytes(&mode(&restored, 1, 0)), bytes(&white));
        for oldmode in [0, 1, 2, 3, 4, 9, 10, 11, 12, 15] {
            let expected = bytes(&mode(&white, 1, oldmode));
            let older = mode(&restored, 1, oldmode);
            assert_eq!(bytes(&older), expected);
            let oldinner = if oldmode == 0 {
                13
            } else if oldmode <= 2 {
                26
            } else if oldmode <= 9 {
                27
            } else {
                30
            };
            assert_eq!(&expected[4..12], words(&[29, oldinner]));
            assert_eq!(
                bytes(&outside(&older, 1, 0)),
                bytes(&mode(&base, 1, oldmode))
            );
        }
        for index in 0..2 {
            assert_eq!(
                restored.layer_source(index).unwrap().to_rgba16().ok(),
                base.layer_source(index).unwrap().to_rgba16().ok()
            );
        }
        assert_eq!(restored.metadata(), base.metadata());
        assert_eq!(restored.resolution(), base.resolution());
        assert_eq!(restored.profile().bytes(), base.profile().bytes());
        assert_eq!(
            restored.layer(1).unwrap().mask().unwrap().plane().samples(),
            [0, 123, 32769, 65535]
        );
        assert_eq!(
            restored
                .layer(1)
                .unwrap()
                .mask()
                .unwrap()
                .properties()
                .outside_coverage,
            255
        );
        assert_eq!(
            restored.render(LIMIT, 0, None).unwrap().to_rgba16().ok(),
            combined.render(LIMIT, 0, None).unwrap().to_rgba16().ok()
        );
    }
}
#[test]
fn combined_hidden_tables_refuse_before_pixels_and_preserve_cancellation_capacity() {
    for actual_mode in [10, 18, 23, 21] {
        let version = match actual_mode {
            10 => 30,
            18 => 31,
            21 => 34,
            _ => 33,
        };
        let original = outside(&fixture(), 1, 255);
        let doc = edit(
            &mode(&original, 1, actual_mode),
            json!({"op":"layer_visible","layer":1,"visible":false}),
        );
        let good = bytes(&doc);
        let blend = tag(&good, b"LBLD");
        let mask = tag(&good, b"MOUT");
        let idat = tag(&good, b"IDAT");
        assert!(blend < mask);
        for (offset, value, reason) in [
            (mask + 4, 2u32, "mask outside schema"),
            (mask + 8, 0, "mask outside count"),
            (mask + 12, 0, "has no mask"),
            (mask + 12, 2, "index"),
            (blend + 4, 2, "blend schema"),
            (blend + 8, 0, "blend count"),
            (
                blend + 16,
                match actual_mode {
                    10 => 13,
                    21 => 27,
                    _ => 21,
                },
                "blend mode",
            ),
            (8, 29, "mask outside typed body"),
            (12, 29, "blending body"),
        ] {
            let mut bad = good.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            bad[idat + 4] ^= 255;
            let err = PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
                .err()
                .unwrap();
            assert!(err.contains(reason), "{offset}: {err}");
        }
        // Opposite wrapper order is never a second accepted spelling.
        let mut reversed = good.clone();
        reversed[4..12].copy_from_slice(&words(&[version, 29]));
        reversed[idat + 4] ^= 255;
        assert!(PrecisionDocument::parse_native(&reversed, LIMIT, 0, None)
            .err()
            .unwrap()
            .contains("blending body"));
        for (oldversion, schema) in [(26u32, 1u32), (27, 2), (30, 3), (31, 4), (33, 5)] {
            if oldversion >= version {
                continue;
            }
            let mut bad = good.clone();
            bad[8..12].copy_from_slice(&oldversion.to_le_bytes());
            bad[blend + 4..blend + 8].copy_from_slice(&schema.to_le_bytes());
            bad[idat + 4] ^= 255;
            assert!(PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
                .err()
                .unwrap()
                .contains("blend mode"));
        }
        for end in mask..good.len() {
            assert!(PrecisionDocument::parse_native(&good[..end], LIMIT, 0, None).is_err());
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(PrecisionDocument::parse_native(&trailing, LIMIT, 0, None).is_err());
        assert!(PrecisionDocument::parse_native(&good, 1, 0, None).is_err());
        assert!(doc.encode_native(1, 0, None).is_err());
        let cancel = DrJobControl::default();
        cancel.cancel();
        assert!(PrecisionDocument::parse_native(&good, LIMIT, 0, Some(&cancel)).is_err());
        assert_eq!(cancel.scratch_bytes(), 0);
        let cancel = DrJobControl::default();
        cancel.cancel();
        assert!(doc.encode_native(LIMIT, 0, Some(&cancel)).is_err());
        assert_eq!(cancel.scratch_bytes(), 0);
        assert_eq!(bytes(&doc), good);
    }
}

#[test]
fn combined_channel_artwork_and_retained_tails_keep_full_state() {
    let base = fixture();
    let source = PrecisionDocument::from_image(
        base.layer_source(0).unwrap().clone(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let place=SourceEdit::parse(&json!({"op":"place","source_id":"wrapped-master","revision_id":"v1","name":"Retained","x":-1,"y":0}).to_string()).unwrap();
    let placed = base
        .source_edit_input(Some(SourceInput::Typed(&source)), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let name = CString::new("Artwork").unwrap();
    let description =
        CString::new(r#"{"type":"shape","version":3,"probe":"combined wrappers"}"#).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        doctored_core::ffi_artwork::dr_precision_doc_artwork(
            &placed,
            0,
            true,
            base.layer_source(0).unwrap(),
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null() && !raw.is_null());
    let artwork = *unsafe { Box::from_raw(raw) };
    let command =
        MaskEdit::parse(r#"{"op":"set_channel","index":null,"name":"Auxiliary"}"#).unwrap();
    let channel = artwork
        .mask_edit(
            &command,
            &[1, 2, 3, 4, 123, 32769, 65535, 0, 11, 12, 13, 14],
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let decorated = edit(
        &channel,
        json!({"op":"channel_properties","index":0,"overlay_color":[17,83,149],"overlay_opacity":0.375}),
    );
    let masked = outside(&decorated, 1, 255);
    let baseline = bytes(&masked);
    for blend_mode in [10, 11, 12, 15, 13, 18, 19, 20, 22] {
        let version = if [10, 11, 12, 15].contains(&blend_mode) {
            30
        } else {
            31
        };
        let combined = mode(&masked, 2, blend_mode);
        let data = bytes(&combined);
        assert_eq!(&data[4..20], words(&[29, version, 25, 20]));
        assert!(tag(&data, b"ADSC") < tag(&data, b"CHPR"));
        assert!(tag(&data, b"CHPR") < tag(&data, b"LBLD"));
        assert!(tag(&data, b"LBLD") < tag(&data, b"MOUT"));
        let again = reopen(&data);
        assert_eq!(bytes(&again), data);
        assert_eq!(bytes(&mode(&again, 2, 0)), baseline);
        let plain = outside(&again, 1, 0);
        assert_eq!(&bytes(&plain)[4..16], words(&[version, 25, 20]));
        assert_eq!(again.source_info(2), decorated.source_info(2));
        assert_eq!(
            again.artwork_description(0),
            decorated.artwork_description(0)
        );
        assert_eq!(
            again.channels()[0].plane().samples(),
            decorated.channels()[0].plane().samples()
        );
        assert_eq!(
            again.channels()[0].metadata(),
            decorated.channels()[0].metadata()
        );
        for i in 0..3 {
            assert_eq!(
                again.layer_source(i).unwrap().to_rgba16().ok(),
                decorated.layer_source(i).unwrap().to_rgba16().ok()
            );
        }
        assert_eq!(again.metadata(), decorated.metadata());
        assert_eq!(again.profile().bytes(), decorated.profile().bytes());
    }
    let group = masked.group_all_pass_through(0.75).unwrap().unwrap();
    let group = mode(&group, 3, 11);
    let before = bytes(&group);
    let group = reopen(&before);
    assert_eq!(bytes(&group), before);
    let passing = mode(&group, 3, 27);
    assert_eq!(&bytes(&passing)[4..16], words(&[29, 25, 20]));
    assert_eq!(bytes(&mode(&passing, 3, 11)), before);
}
