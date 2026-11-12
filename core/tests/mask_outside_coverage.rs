//! Constant raw mask backgrounds: independent scalar, geometry, and wire oracles.
//! The scoped scalar writeback still edits finite raw storage, preserving background;
//! Invert over the whole canvas is `mask_invert_whole.rs`.
use crate::common;
use common::{identity_tail, take_err_string, without_identity, FILTER_BILINEAR};
use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::doc_plane::Plane;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_mask::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::precision_plane_read::PlaneRead;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::json;
use std::ffi::CString;
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn byte(outside: u8, value: u8, feather: f32) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(9, 7, Rgba([31, 72, 119, 255])));
    let layer = &mut doc.layers[0];
    layer.mask = Some(Arc::new(GrayImage::from_pixel(1, 1, Luma([value]))));
    layer.mask_properties = MaskProperties::at((4, 3));
    layer.mask_properties.outside_coverage = outside;
    layer.mask_properties.feather = feather;
    doc
}
fn effective(doc: &DrDocument) -> Vec<u8> {
    doc.layer_effective_mask_image(0)
        .expect("an effective mask")
        .pixels()
        .map(|p| p[0])
        .collect()
}
fn typed(outside: u8, value: u16, feather: f32) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        9,
        7,
        vec![[13001, 22002, 31003, 65535]; 63],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let mut p = MaskProperties::at((4, 3));
    p.outside_coverage = outside;
    p.feather = feather;
    doc.set_layer_mask(
        0,
        Some(
            PrecisionMask::new(
                PrecisionPlane::from_u16(1, 1, vec![value], None).unwrap(),
                p,
                true,
            )
            .unwrap(),
        ),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}
fn read(doc: &PrecisionDocument, target: &str, canvas: bool) -> Vec<u16> {
    PlaneRead::parse(
        &json!({"op":"read_plane","target":target,"index":0,"canvas":canvas}).to_string(),
    )
    .unwrap()
    .read(doc, LIMIT, 0, None)
    .unwrap()
}
fn byte_bytes(doc: &DrDocument) -> Vec<u8> {
    let temp = TempDir::new().unwrap();
    let path = CString::new(temp.path().join("mask.rz").to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, path.as_ptr(), std::ptr::null(), &mut error) },
        "{}",
        take_err_string(error)
    );
    std::fs::read(path.to_str().unwrap()).unwrap()
}
fn byte_parse(bytes: &[u8]) -> Result<Box<DrDocument>, String> {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("mask.rz");
    std::fs::write(&path, bytes).unwrap();
    let path = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let handle = unsafe { dr_doc_open(path.as_ptr(), std::ptr::null(), &mut error) };
    if handle.is_null() {
        Err(take_err_string(error))
    } else {
        Ok(unsafe { Box::from_raw(handle) })
    }
}
fn word(bytes: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(bytes[index..index + 4].try_into().unwrap())
}
/// …and with the `MOUT` wrapper's own word and `count`-index table removed
/// as well: what the writer before either wrapper produced.
fn old_body(bytes: &[u8], count: usize, records: usize) -> Vec<u8> {
    let prior = without_identity(bytes, records);
    [&prior[..4], &prior[8..prior.len() - 12 - count * 4]].concat()
}

/// Where the `MOUT` table starts.
fn mout_at(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"MOUT").expect("MOUT")
}

#[test]
fn byte_raw_effective_render_and_shared_cache_keys_include_background() {
    let black = byte(0, 73, 0.0);
    assert_eq!(effective(&black)[0], 0);
    black.flattened().unwrap(); // populate aligned cache
    let mut white = black.clone(); // same raw identity, same derived cache owners
    white.layers[0].mask_properties.outside_coverage = 255;
    assert_eq!(effective(&white)[0], 255);
    assert_eq!(white.layer_plane(0, Plane::Mask).unwrap()[0], 255);
    assert_eq!(white.flattened().unwrap().get_pixel(0, 0)[3], 255);
    assert_eq!(black.flattened().unwrap().get_pixel(0, 0)[3], 0);
    assert!(!unsafe { dr_doc_layer_masks_equal(&black, &white, 0) });
    let metadata = take_err_string(unsafe { dr_doc_layer_mask_properties(&white, 0) });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&metadata).unwrap()["outside_coverage"],
        255
    );
    let handle = unsafe { dr_doc_set_mask_properties(&white, 0, false, 0.625, 1.0) };
    assert!(!handle.is_null());
    let changed = unsafe { Box::from_raw(handle) };
    assert_eq!(changed.layers[0].mask_properties.outside_coverage, 255);
    assert!(Arc::ptr_eq(
        changed.layers[0].mask.as_ref().unwrap(),
        white.layers[0].mask.as_ref().unwrap()
    ));
    assert_eq!(effective(&changed)[0], 255);
    assert_eq!(effective(&white)[3 * 9 + 4], 73);
}

#[test]
fn byte_and_native_feather_match_independent_gaussian_complement_and_far_field() {
    // Radius1 uses three taps in each dimension. Independently evaluate a
    // single dark sample against a white infinite background, including byte
    // rounding after each separable pass and native continuous coverage.
    let side = (-0.5_f64 / 0.8_f64.powi(2)).exp();
    let weights = [
        side / (1.0 + 2.0 * side),
        1.0 / (1.0 + 2.0 * side),
        side / (1.0 + 2.0 * side),
    ];
    let b = byte(255, 17, 1.0);
    let t = typed(255, 1234, 1.0);
    let bv = effective(&b);
    let tv = read(&t, "effective_mask", true);
    for y in 0..7 {
        for x in 0..9 {
            let wantb = if (3..=5).contains(&x) && (2..=4).contains(&y) {
                255 - ((238.0 * weights[x - 3]).round() * weights[y - 2]).round() as u8
            } else {
                255
            };
            let wantt = if (3..=5).contains(&x) && (2..=4).contains(&y) {
                (65535.0 - 64301.0 * weights[x - 3] * weights[y - 2]).round() as u16
            } else {
                65535
            };
            assert_eq!(bv[y * 9 + x], wantb, "byte {x},{y}");
            assert!(
                tv[y * 9 + x].abs_diff(wantt) <= 1,
                "native {x},{y}: {} != {wantt}",
                tv[y * 9 + x]
            );
        }
    }
    // Large radius takes the downsampled zero-extended workspace. A white
    // constant field must remain white through that path and around its edge.
    for feather in [1.0, 31.0, 100.0] {
        assert!(effective(&byte(255, 255, feather))
            .iter()
            .all(|&v| v == 255));
        assert!(read(&typed(255, 65535, feather), "effective_mask", true)
            .iter()
            .all(|&v| v == 65535));
    }
    // Read across the far padding boundary, not only near the original cell.
    // The old fill-white/zero-extend blur would introduce a seam here.
    let mut far = byte(255, 255, 100.0);
    far.width = 400;
    far.height = 3;
    far.layers[0].mask_properties.offset = (200, 1);
    assert!(effective(&far).iter().all(|&v| v == 255));
    let source = typed(255, 65535, 100.0);
    let leaf = source.layer(0).unwrap().clone();
    let mask = leaf.mask().unwrap();
    let mut properties = mask.properties().clone();
    properties.offset = (200, 1);
    let mask = mask.with_properties(properties, true).unwrap();
    let far = PrecisionDocument::from_parts(
        400,
        3,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![leaf.with_mask(Some(mask))],
    )
    .unwrap();
    assert!(read(&far, "effective_mask", true)
        .iter()
        .all(|&v| v == 65535));
    // Same raw Arc, changed outside and unchanged feather cannot reuse stale derivation.
    let zero = byte(0, 17, 1.0);
    let original = effective(&zero);
    let mut aliased = zero.clone();
    aliased.layers[0].mask_properties.outside_coverage = 255;
    assert_eq!(effective(&aliased), bv);
    assert_eq!(effective(&zero), original);
}

#[test]
fn byte_and_native_fractional_affine_sample_background_and_retain_exact_source() {
    let b = byte(255, 17, 0.0);
    let matrix = [1.0, 0.0, 0.0, 1.0, 0.25, 0.0];
    let handle = unsafe { dr_doc_transform_mask(&b, 0, matrix.as_ptr(), FILTER_BILINEAR) };
    assert!(!handle.is_null());
    let moved = unsafe { Box::from_raw(handle) };
    assert_eq!(moved.layers[0].mask.as_ref().unwrap().as_raw(), &[77, 196]);
    assert_eq!(moved.layers[0].mask_properties.outside_coverage, 255);
    assert!(LayerRaster::ptr_eq(
        &b.layers[0].pixels,
        &moved.layers[0].pixels
    ));
    let t = typed(255, 1234, 0.0);
    let geometry = GeometryEdit::parse(
        &json!({"op":"affine","layer":0,"target":"mask","matrix":matrix,"filter":"bilinear"})
            .to_string(),
    )
    .unwrap();
    let moved = t.geometry(&geometry, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(read(&moved, "mask", false), vec![17309, 49460]);
    assert_eq!(read(&moved, "mask", true)[0], 65535);
    assert_eq!(
        t.layer_source(0).unwrap().to_rgba16().ok(),
        moved.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(
        moved
            .layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .properties()
            .outside_coverage,
        255
    );
    let rotation = t
        .geometry(&GeometryEdit::Rotate90, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(read(&rotation, "mask", false), vec![1234]);
    assert_eq!(
        rotation
            .layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .properties()
            .outside_coverage,
        255
    );
    let b = b.rotate90().unwrap();
    assert_eq!(b.layers[0].mask_properties.outside_coverage, 255);
}

#[test]
fn typed_property_edits_disabled_masks_and_raw_reads_preserve_default() {
    let t = typed(255, 1234, 0.0);
    let command=MaskEdit::parse(&json!({"op":"mask_properties","layer":0,"x":-20,"y":3,"linked":false,"density":0.25,"feather":1.0,"enabled":false}).to_string()).unwrap();
    let next = t.mask_edit(&command, &[], LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(
        next.layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .properties()
            .outside_coverage,
        255
    );
    assert_eq!(read(&next, "mask", false), vec![1234]);
    assert!(read(&next, "mask", true).iter().all(|&v| v == 65535));
    assert!(next
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .all(|p| p[3] == 65535));
    assert!(next
        .mask_edit(&command, &[], LIMIT, 0, None)
        .unwrap()
        .is_none());
    let control = DrJobControl::default();
    control.cancel();
    assert!(t
        .mask_edit(&command, &[], LIMIT, 0, Some(&control))
        .is_err());
    assert_eq!(
        t.layer(0).unwrap().mask().unwrap().properties().offset,
        (4, 3)
    );
}

#[test]
fn conditional_wrappers_retain_exact_prior_bodies_and_roundtrip_raw_metadata() {
    let b0 = byte(0, 17, 1.0);
    let b1 = byte(255, 17, 1.0);
    let old = byte_bytes(&b0);
    let new = byte_bytes(&b1);
    // Since 12E-1 the outermost word is the always-written identity wrapper
    // (45 byte / 46 typed) and its `LIDS` table closes the file, so the
    // prior body is what remains once BOTH wrappers come off.
    assert_eq!(word(&old, 4), 45);
    assert_eq!(word(&old, 8), 10);
    assert_eq!(word(&new, 4), 45);
    assert_eq!(word(&new, 8), 28);
    assert_eq!(word(&new, 12), 10);
    assert_eq!(old_body(&new, 1, 1), without_identity(&old, 1));
    let at = mout_at(&new);
    assert_eq!(&new[at..at + 16], b"MOUT\x01\0\0\0\x01\0\0\0\0\0\0\0");
    let back = byte_parse(&new).unwrap();
    assert_eq!(byte_bytes(&back), new);
    assert_eq!(effective(&back), effective(&b1));
    let t0 = typed(0, 1234, 1.0);
    let t1 = typed(255, 1234, 1.0);
    let old = t0.encode_native(LIMIT, 0, None).unwrap();
    let new = t1.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!(word(&old, 4), 46);
    assert_eq!(word(&old, 8), 13);
    assert_eq!(word(&new, 4), 46);
    assert_eq!(word(&new, 8), 29);
    assert_eq!(word(&new, 12), 13);
    assert_eq!(old_body(&new, 1, 1), without_identity(&old, 1));
    let back = PrecisionDocument::parse_native(&new, LIMIT, 0, None).unwrap();
    assert_eq!(back.encode_native(LIMIT, 0, None).unwrap(), new);
    assert_eq!(read(&back, "mask", false), vec![1234]);
    assert_eq!(
        read(&back, "effective_mask", true),
        read(&t1, "effective_mask", true)
    );
    let control = DrJobControl::default();
    control.cancel();
    assert!(PrecisionDocument::parse_native(&new, LIMIT, 0, Some(&control)).is_err());
    assert!(t1.encode_native(1, 0, None).is_err());
}

#[test]
fn strict_mask_tables_refuse_versions_counts_indexes_missing_masks_and_trailing_bytes() {
    for typed_case in [false, true] {
        let good = if typed_case {
            typed(255, 1234, 0.0).encode_native(LIMIT, 0, None).unwrap()
        } else {
            byte_bytes(&byte(255, 17, 0.0))
        };
        let tail = mout_at(&good);
        for (position, value) in [
            (12, 0),
            (12, 28),
            (tail + 4, 2),
            (tail + 8, 0),
            (tail + 8, 2),
            (tail + 12, 1),
        ] {
            let mut bad = good.clone();
            bad[position..position + 4].copy_from_slice(&u32::to_le_bytes(value));
            let error = if typed_case {
                PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
                    .err()
                    .unwrap()
            } else {
                byte_parse(&bad).err().unwrap()
            };
            assert!(error.contains("mask outside"), "{error}");
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(if typed_case {
            PrecisionDocument::parse_native(&trailing, LIMIT, 0, None).is_err()
        } else {
            byte_parse(&trailing).is_err()
        });
        // Corrupt PNG inflation too: malformed MOUT must be rejected first.
        let mut bad = good.clone();
        bad[tail + 4..tail + 8].copy_from_slice(&2u32.to_le_bytes());
        let idat = bad.windows(4).position(|b| b == b"IDAT").unwrap();
        bad[idat + 4] ^= 0xff;
        let error = if typed_case {
            PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
                .err()
                .unwrap()
        } else {
            byte_parse(&bad).err().unwrap()
        };
        assert!(error.contains("mask outside schema"), "{error}");
        let no_mask = if typed_case {
            let t = typed(0, 1234, 0.0);
            t.set_layer_mask(0, None, LIMIT, 0, None)
                .unwrap()
                .unwrap()
                .encode_native(LIMIT, 0, None)
                .unwrap()
        } else {
            let mut b = byte(0, 17, 0.0);
            b.layers[0].mask = None;
            byte_bytes(&b)
        };
        // The identity wrapper is rebuilt around the crafted body: its word
        // outermost and its `LIDS` tail last, exactly where the writer puts
        // them.
        let identity = good[good.len() - identity_tail(1)..].to_vec();
        let mut no_mask = without_identity(&no_mask, 1);
        let wrapped = [
            b"RZDC".as_slice(),
            &u32::to_le_bytes(if typed_case { 46 } else { 45 }),
            &u32::to_le_bytes(if typed_case { 29 } else { 28 }),
            &no_mask.split_off(4),
            &good[tail..good.len() - identity_tail(1)],
            &identity,
        ]
        .concat();
        let error = if typed_case {
            PrecisionDocument::parse_native(&wrapped, LIMIT, 0, None)
                .err()
                .unwrap()
        } else {
            byte_parse(&wrapped).err().unwrap()
        };
        assert!(error.contains("reference has no mask"), "{error}");
    }
}

#[test]
fn group_masks_keep_postorder_references_and_nonendpoint_properties_refuse() {
    let t = typed(255, 1234, 0.0);
    let leaf = t.layer(0).unwrap().clone();
    let mask = leaf.mask().unwrap().clone();
    let group = PrecisionDocumentLayer::pass_through("Group", vec![leaf], 0.5, true)
        .unwrap()
        .with_mask(Some(mask));
    let grouped = PrecisionDocument::from_parts(
        9,
        7,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![group],
    )
    .unwrap();
    let bytes = grouped.encode_native(LIMIT, 0, None).unwrap();
    // The `MOUT` table is located by its tag: `LIDS` closes the file now.
    let at = mout_at(&bytes);
    assert_eq!(word(&bytes, at + 8), 2);
    assert_eq!(word(&bytes, at + 12), 0);
    assert_eq!(word(&bytes, at + 16), 1);
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened
            .layer(1)
            .unwrap()
            .mask()
            .unwrap()
            .properties()
            .outside_coverage,
        255
    );
    assert_eq!(
        reopened.render(LIMIT, 0, None).unwrap().to_rgba16().ok(),
        grouped.render(LIMIT, 0, None).unwrap().to_rgba16().ok()
    );
    let mut p = MaskProperties::default();
    p.outside_coverage = 127;
    assert!(PrecisionMask::new(
        PrecisionPlane::from_u16(1, 1, vec![0], None).unwrap(),
        p,
        true
    )
    .is_err());
    let bad = byte(127, 0, 0.0);
    let temp = TempDir::new().unwrap();
    assert!(bad
        .save_native(temp.path().join("bad.rz").to_str().unwrap())
        .is_err());
}

#[test]
fn styled_shape_and_content_bounds_caches_observe_same_raw_background_change() {
    let hidden = byte(0, 0, 0.0);
    let style =
        CString::new(r##"{"effects":[{"type":"color_overlay","color":"#00ff00","opacity":1}]}"##)
            .unwrap();
    let mut error = ptr::null_mut();
    let pointer = unsafe {
        doctored_core::ffi_style::dr_doc_set_layer_style(&hidden, 0, style.as_ptr(), &mut error)
    };
    assert!(!pointer.is_null(), "{}", take_err_string(error));
    let hidden = unsafe { Box::from_raw(pointer) };
    let old = hidden.flattened().unwrap();
    assert!(old.pixels().all(|p| p[3] == 0));
    assert!(hidden.layer_bounds(0).is_none());
    let mut shown = hidden.as_ref().clone();
    shown.layers[0].mask_properties.outside_coverage = 255;
    let rendered = shown.flattened().unwrap();
    assert_eq!(*rendered.get_pixel(0, 0), Rgba([0, 255, 0, 255]));
    assert!(shown.layer_bounds(0).is_some());
    assert_eq!(hidden.flattened().unwrap(), old);
    assert!(hidden.layer_bounds(0).is_none());
}
