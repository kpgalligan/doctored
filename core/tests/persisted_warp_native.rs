//! The persisted warp (`WARP`, DRDC 53 byte / 54 typed) at both depths.
//!
//! # The stored 51/52-writer files
//!
//! `tests/fixtures/rz_byte_51/warp_fixture.rz` and
//! `tests/fixtures/rz_typed_52/warp_fixture.rz` are FORMAT ORACLES, as
//! `psd_10c/*.psd` is — not golden images. They were written by the writers
//! that predate words 53 and 54, never by this build, and nothing in this
//! tree regenerates them:
//!
//! - commit: 07d915344bc44163a0f1a57d60d528838fe37839 (07d9153, `6400648^`,
//!   the last writers without the warp words), checked out in a scratch
//!   `git worktree`;
//! - builder: a throwaway, never-committed `core/tests/gen_rz51.rs` holding
//!   `common/warp_fixture.rs`'s constants, pixel ladders, `byte_document` and
//!   `typed_document` verbatim (not `field`/`edit`, which need `WarpEdit`)
//!   and two ignored tests: `byte_document().save_native($DR_OUT)` and
//!   `typed_document().encode_native(LIMIT, 0, None)` written to `$DR_OUT`;
//! - command: `cd <worktree>/core && CARGO_TARGET_DIR=<scratch> DR_OUT=<repo>/core/tests/fixtures/rz_byte_51/warp_fixture.rz
//!   cargo test --release --test gen_rz51 -- --ignored write_rz51`, and the
//!   same with `rz_typed_52/warp_fixture.rz` and `write_rz52`;
//! - bytes: 1105, sha256
//!   `75a7c4a66a27629fb7fe6e0f7fb33df1ff7a488953bc599624c9f261844afcf5`
//!   (byte); 4541, sha256
//!   `9315e417f6e2a5d1bcb81f989dfba824150965f0f193cd30a2519e88cc27308a`
//!   (typed).
//!
//! The current writers must reproduce them byte for byte. The planes inside
//! them are encoded by the PNG crate, so a dependency bump that moves the PNG
//! encoder's bytes fails those tests ON PURPOSE: the stored files are what an
//! older build wrote, and a writer that no longer reproduces them has changed
//! the format.

use crate::identity;
use crate::warp_fixture as fixture;

use doctored_core::doc::DrDocument;
use doctored_core::doc::LayerRaster;
use doctored_core::doc_comps::CompRequest;
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::precision_paint::PaintEdit;
use doctored_core::retained_source::SourceEdit;
use fixture::{byte_document, edit, field, typed_document, LIMIT};
use serde_json::json;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn conditional_warp_is_nested_inside_identity_and_round_trips_at_both_depths() {
    let dir = tempdir().unwrap();
    let before = byte_document();
    let byte_path = dir.path().join("byte.rz");
    before.save_native(byte_path.to_str().unwrap()).unwrap();
    let original = fs::read(&byte_path).unwrap();
    let request = edit(field(3, 3, "bicubic"));
    let set = before.warp_edit(&request, LIMIT, 0, None).unwrap().unwrap();
    set.save_native(byte_path.to_str().unwrap()).unwrap();
    let saved = fs::read(&byte_path).unwrap();
    assert_eq!(&saved[..8], b"DRDC\x2d\0\0\0");
    assert_eq!(&saved[8..12], &53u32.to_le_bytes());
    assert!(saved.windows(4).any(|w| w == b"WARP"));
    assert!(saved.windows(4).last().is_some());
    let reopened = DrDocument::open(byte_path.to_str().unwrap()).unwrap();
    assert!(reopened.warp_info(0).unwrap()["field"].is_object());
    reopened.save_native(byte_path.to_str().unwrap()).unwrap();
    assert_eq!(saved, fs::read(&byte_path).unwrap());
    let cleared = reopened
        .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    cleared.save_native(byte_path.to_str().unwrap()).unwrap();
    assert_eq!(original, fs::read(&byte_path).unwrap());
    let baked = reopened
        .warp_edit(&edit(json!({"op":"bake","layer":0})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(baked.warp_info(0).unwrap()["field"].is_null());
    baked.save_native(byte_path.to_str().unwrap()).unwrap();
    let baked_bytes = fs::read(&byte_path).unwrap();
    assert!(!baked_bytes.windows(4).any(|w| w == b"WARP"));
    assert_ne!(baked_bytes, original);

    let before = typed_document();
    let typed_path = dir.path().join("typed.rz");
    before
        .save_native(typed_path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let original = fs::read(&typed_path).unwrap();
    let set = before.warp_edit(&request, LIMIT, 0, None).unwrap().unwrap();
    set.save_native(typed_path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let saved = fs::read(&typed_path).unwrap();
    assert_eq!(&saved[..8], b"DRDC\x2e\0\0\0");
    assert_eq!(&saved[8..12], &54u32.to_le_bytes());
    assert!(saved.windows(4).any(|w| w == b"WARP"));
    let reopened =
        PrecisionDocument::open_native(typed_path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(reopened.warp_info(0).unwrap()["field"].is_object());
    reopened
        .save_native(typed_path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    assert_eq!(saved, fs::read(&typed_path).unwrap());
    let cleared = reopened
        .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    cleared
        .save_native(typed_path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    assert_eq!(original, fs::read(&typed_path).unwrap());
    let baked = reopened
        .warp_edit(&edit(json!({"op":"bake","layer":0})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    baked
        .save_native(typed_path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    assert!(!fs::read(&typed_path)
        .unwrap()
        .windows(4)
        .any(|w| w == b"WARP"));
}

#[test]
fn reopened_pixel_reedit_uses_original_and_destructive_reopen_cannot_restore_old_base() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("pixel-reedit.rz");
    let a = edit(field(3, 3, "bilinear"));
    let b = edit(field(4, 2, "bicubic"));
    let clear = edit(json!({"op":"clear","layer":0}));

    let original = byte_document();
    let first = original.warp_edit(&a, LIMIT, 0, None).unwrap().unwrap();
    first.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    let reedited = reopened.warp_edit(&b, LIMIT, 0, None).unwrap().unwrap();
    let fresh = original.warp_edit(&b, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(reedited.warp_info(0).unwrap(), fresh.warp_info(0).unwrap());
    assert_eq!(reedited.layers[0].offset, fresh.layers[0].offset);
    assert_eq!(
        reedited.layers[0].pixels.contiguous_image().as_raw(),
        fresh.layers[0].pixels.contiguous_image().as_raw()
    );
    let restored = reedited.warp_edit(&clear, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(
        restored.layers[0].pixels.contiguous_image().as_raw(),
        original.layers[0].pixels.contiguous_image().as_raw()
    );
    assert_eq!(restored.layers[0].offset, original.layers[0].offset);

    let replacement = image::RgbaImage::from_pixel(
        first.layers[0].pixels.width(),
        first.layers[0].pixels.height(),
        image::Rgba([251, 17, 93, 63]),
    );
    let destructed = first.with_layer_pixels(0, replacement).unwrap();
    assert!(destructed.warp_info(0).unwrap()["field"].is_null());
    destructed.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert!(reopened.warp_info(0).unwrap()["field"].is_null());
    assert!(reopened
        .warp_edit(&clear, 1, u64::MAX, None)
        .unwrap()
        .is_none());
    let rebased = reopened.warp_edit(&b, LIMIT, 0, None).unwrap().unwrap();
    let cleared = rebased.warp_edit(&clear, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(
        cleared.layers[0].pixels.contiguous_image().as_raw(),
        destructed.layers[0].pixels.contiguous_image().as_raw()
    );
    assert_eq!(cleared.layers[0].offset, destructed.layers[0].offset);
    assert_ne!(
        cleared.layers[0].pixels.contiguous_image().as_raw(),
        original.layers[0].pixels.contiguous_image().as_raw()
    );

    let original = typed_document();
    let first = original.warp_edit(&a, LIMIT, 0, None).unwrap().unwrap();
    first
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    let reedited = reopened.warp_edit(&b, LIMIT, 0, None).unwrap().unwrap();
    let fresh = original.warp_edit(&b, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(reedited.warp_info(0).unwrap(), fresh.warp_info(0).unwrap());
    assert_eq!(
        reedited.layer(0).unwrap().offset(),
        fresh.layer(0).unwrap().offset()
    );
    assert_eq!(
        reedited.layer_source(0).unwrap().to_rgba16().ok(),
        fresh.layer_source(0).unwrap().to_rgba16().ok()
    );
    let restored = reedited.warp_edit(&clear, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(
        restored.layer_source(0).unwrap().to_rgba16().ok(),
        original.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(
        restored.layer(0).unwrap().offset(),
        original.layer(0).unwrap().offset()
    );

    let erase = PaintEdit::parse(r#"{"op":"erase","layer":0,"opacity":1.0}"#).unwrap();
    let destructed = first
        .paint(
            &erase,
            &vec![65535; (fixture::W * fixture::H) as usize],
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(destructed.warp_info(0).unwrap()["field"].is_null());
    destructed
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(reopened.warp_info(0).unwrap()["field"].is_null());
    assert!(reopened
        .warp_edit(&clear, 1, u64::MAX, None)
        .unwrap()
        .is_none());
    let rebased = reopened.warp_edit(&b, LIMIT, 0, None).unwrap().unwrap();
    let cleared = rebased.warp_edit(&clear, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(
        cleared.layer_source(0).unwrap().to_rgba16().ok(),
        destructed.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(
        cleared.layer(0).unwrap().offset(),
        destructed.layer(0).unwrap().offset()
    );
    assert_ne!(
        cleared.layer_source(0).unwrap().to_rgba16().ok(),
        original.layer_source(0).unwrap().to_rgba16().ok()
    );
}

#[test]
fn malformed_warp_table_is_refused_before_fallback_decode() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bad.rz");
    let set = byte_document()
        .warp_edit(&edit(field(3, 3, "bilinear")), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    set.save_native(path.to_str().unwrap()).unwrap();
    let saved = fs::read(&path).unwrap();
    let at = saved.windows(4).position(|w| w == b"WARP").unwrap();
    for (relative, value, needle) in [
        (0, b'X', "WARP"),
        (4, 2, "schema"),
        (12, 255, "indices"),
        (16, 3, "target"),
        (17, 3, "filter"),
        (18, 2, "link"),
        (19, 17, "outside"),
    ] {
        let mut bad = saved.clone();
        bad[at + relative] = value;
        fs::write(&path, bad).unwrap();
        let error = DrDocument::open(path.to_str().unwrap()).err().unwrap();
        assert!(error.contains(needle), "{relative}: {error}");
    }
}

fn poisoned_fallback(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let idat = out.windows(4).position(|w| w == b"IDAT").unwrap();
    // Keep the PNG signature and IHDR readable for native record preflight;
    // the compressed stream itself must fail if WARP admission ever falls through.
    out[idat + 8] ^= 0xff;
    out
}

fn put_u32(bytes: &mut [u8], at: usize, n: u32) {
    bytes[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn put_u64(bytes: &mut [u8], at: usize, n: u64) {
    bytes[at..at + 8].copy_from_slice(&n.to_le_bytes());
}

#[test]
fn byte_and_typed_warp_preflight_rejects_malformed_wire_before_poisoned_png() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mutation.rz");
    for depth in [8, 16] {
        let request = edit(field(3, 3, "bilinear"));
        if depth == 8 {
            byte_document()
                .warp_edit(&request, LIMIT, 0, None)
                .unwrap()
                .unwrap()
                .save_native(path.to_str().unwrap())
                .unwrap();
        } else {
            typed_document()
                .warp_edit(&request, LIMIT, 0, None)
                .unwrap()
                .unwrap()
                .save_native(path.to_str().unwrap(), LIMIT, 0, None)
                .unwrap();
        }
        let original = fs::read(&path).unwrap();
        let at = original.windows(4).position(|w| w == b"WARP").unwrap();
        let plane = at + 28 + 9 * 16;
        let length = u64::from_le_bytes(original[plane + 17..plane + 25].try_into().unwrap());
        let invalid_png = poisoned_fallback(&original);
        fs::write(&path, &invalid_png).unwrap();
        let decode_error = if depth == 8 {
            DrDocument::open(path.to_str().unwrap()).err().unwrap()
        } else {
            PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
                .err()
                .unwrap()
        };
        let shown = decode_error.replace(&dir.path().to_string_lossy().into_owned(), "<tmp>");
        assert!(
            shown.to_lowercase().contains("png") || shown.to_lowercase().contains("decode"),
            "{decode_error}"
        );
        let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
        let mut change = |name: &'static str, f: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = invalid_png.clone();
            f(&mut bytes);
            cases.push((name, bytes));
        };
        change("tag", &|b| b[at] = b'X');
        change("schema", &|b| put_u32(b, at + 4, 2));
        change("count", &|b| put_u32(b, at + 8, 0));
        change("index", &|b| put_u32(b, at + 12, 999));
        change("target", &|b| b[at + 16] = 2);
        change("filter", &|b| b[at + 17] = 2);
        change("linked flag", &|b| b[at + 18] = 2);
        change("outside flag", &|b| b[at + 19] = 1);
        change("columns", &|b| put_u32(b, at + 20, 1));
        change("rows", &|b| put_u32(b, at + 24, 17));
        change("NaN", &|b| {
            b[at + 28..at + 36].copy_from_slice(&f64::NAN.to_le_bytes())
        });
        change("contraction", &|b| {
            b[at + 44..at + 52].copy_from_slice(&20f64.to_le_bytes())
        });
        change("identity", &|b| b[at + 28..plane].fill(0));
        change("plane flag", &|b| b[plane] = 2);
        change("plane absence", &|b| b[plane] = 0);
        change("plane dimensions", &|b| put_u32(b, plane + 9, 0));
        change("sample length", &|b| put_u64(b, plane + 17, length + 1));
        change("fallback extent", &|b| put_u32(b, plane + 9, 18));
        if depth == 16 {
            change("float wrapper", &|b| put_u32(b, 12, 40));
        }
        change("truncation", &|b| b.truncate(at + 36));
        change("trailing", &|b| b.push(77));
        for (name, bytes) in cases {
            fs::write(&path, bytes).unwrap();
            let error = if depth == 8 {
                DrDocument::open(path.to_str().unwrap()).err().unwrap()
            } else {
                PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
                    .err()
                    .unwrap()
            };
            let shown = error.replace(&dir.path().to_string_lossy().into_owned(), "<tmp>");
            assert!(
                !shown.to_lowercase().contains("decode") && !shown.to_lowercase().contains("png"),
                "{depth}-bit {name} reached poisoned fallback: {error}"
            );
        }
    }
}

#[test]
fn duplicate_warp_entries_refuse_unsorted_indices_before_png_decode() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("out-of-order.rz");
    for depth in [8, 16] {
        let first = field(3, 3, "bilinear");
        let mut second = first.clone();
        second["layer"] = json!(1);
        if depth == 8 {
            let base = byte_document().duplicate_layers(&[0]).unwrap();
            let both = base
                .warp_edit(&edit(first), LIMIT, 0, None)
                .unwrap()
                .unwrap()
                .warp_edit(&edit(second), LIMIT, 0, None)
                .unwrap()
                .unwrap();
            both.save_native(path.to_str().unwrap()).unwrap();
        } else {
            let base = typed_document().duplicate_layer(0).unwrap().unwrap();
            let both = base
                .warp_edit(&edit(first), LIMIT, 0, None)
                .unwrap()
                .unwrap()
                .warp_edit(&edit(second), LIMIT, 0, None)
                .unwrap()
                .unwrap();
            both.save_native(path.to_str().unwrap(), LIMIT, 0, None)
                .unwrap();
        }
        let mut bytes = poisoned_fallback(&fs::read(&path).unwrap());
        let at = bytes.windows(4).position(|w| w == b"WARP").unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()),
            2
        );
        let first_plane = at + 28 + 9 * 16;
        let length = u64::from_le_bytes(
            bytes[first_plane + 17..first_plane + 25]
                .try_into()
                .unwrap(),
        ) as usize;
        let second_entry = first_plane + 25 + length + 1;
        assert_eq!(
            u32::from_le_bytes(bytes[second_entry..second_entry + 4].try_into().unwrap()),
            1
        );
        put_u32(&mut bytes, second_entry, 0);
        fs::write(&path, bytes).unwrap();
        let error = if depth == 8 {
            DrDocument::open(path.to_str().unwrap()).err().unwrap()
        } else {
            PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
                .err()
                .unwrap()
        };
        assert!(error.contains("WARP indices"), "{depth}-bit: {error}");
    }
}

#[test]
fn pixel_warp_without_original_mask_refuses_linked_fallback_before_png_decode() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("fallback-link.rz");
    let mut before = byte_document();
    let raw: Vec<u8> = (0..11 * 19).map(|i| ((i * 43 + 7) % 256) as u8).collect();
    before.layers[0].mask = Some(Arc::new(image::GrayImage::from_raw(11, 19, raw).unwrap()));
    let mut props = MaskProperties::at((1, -4));
    props.linked = false;
    before.layers[0].mask_properties = props;
    before
        .warp_edit(&edit(field(3, 3, "bilinear")), LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .save_native(path.to_str().unwrap())
        .unwrap();
    let mut bytes = poisoned_fallback(&fs::read(&path).unwrap());
    let png = bytes
        .windows(8)
        .position(|w| w == b"\x89PNG\r\n\x1a\n")
        .unwrap();
    let mut cursor = png + 8;
    loop {
        let length = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        let kind = &bytes[cursor + 4..cursor + 8];
        cursor += 12 + length;
        if kind == b"IEND" {
            break;
        }
    }
    assert_eq!(bytes[cursor], 1, "fallback mask missing");
    let mask_length =
        u32::from_le_bytes(bytes[cursor + 2..cursor + 6].try_into().unwrap()) as usize;
    let mask_link = cursor + 14 + mask_length + 23;
    assert_eq!(bytes[mask_link], 0);
    bytes[mask_link] = 1;
    fs::write(&path, bytes).unwrap();
    let error = DrDocument::open(path.to_str().unwrap()).err().unwrap();
    assert!(error.contains("WARP fallback mask properties"), "{error}");
}

#[test]
fn warp_and_projective_source_tables_compose_and_downgrade_independently() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("composed.rz");
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"warp-other-master",
        "revision_id":"first","name":"Retained", "x":0,"y":0})
        .to_string(),
    )
    .unwrap();
    let geometry = GeometryEdit::parse(
        &json!({"op":"perspective_set","layers":[1],
        "source_rect":[0,0,4,4],"quad":[0,0,20.0/7.0,0,20.0/7.0,20.0/7.0,0,4],
        "filter":"bilinear"})
        .to_string(),
    )
    .unwrap();
    let master = DrDocument::from_pixels(image::RgbaImage::from_fn(4, 4, |x, y| {
        image::Rgba([
            (x * 43 + 1) as u8,
            (y * 37 + 3) as u8,
            199,
            if x == 0 { 0 } else { 255 },
        ])
    }));
    let byte = byte_document()
        .source_edit(Some(&master), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .geometry_edit(&geometry, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let byte = byte.adding_layer(1, "Described").unwrap();
    let mut byte = byte
        .with_layer_label(0, 3)
        .unwrap()
        .with_layer_locks(1, doctored_core::doc_lock::LOCK_POSITION)
        .unwrap()
        .add_guide(GuideOrientation::Vertical, 3.0)
        .unwrap();
    byte.layers[2].meta = Some("opaque newer artwork description".into());
    let comp = CompRequest::parse(
        &json!({"op":"layer_comp","action":"save",
        "name":"Warp composition","visibility":true})
        .to_string(),
    )
    .unwrap();
    let byte = byte.edit_layer_comps(&comp).unwrap().unwrap();
    let byte = byte
        .warp_edit(&edit(field(3, 3, "bicubic")), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    byte.save_native(path.to_str().unwrap()).unwrap();
    let encoded = fs::read(&path).unwrap();
    assert_eq!(&encoded[4..16], &[45, 0, 0, 0, 53, 0, 0, 0, 51, 0, 0, 0]);
    let projective = encoded.windows(4).position(|w| w == b"RSPJ").unwrap();
    let warp = encoded.windows(4).position(|w| w == b"WARP").unwrap();
    let ids = encoded.windows(4).position(|w| w == b"LIDS").unwrap();
    assert!(projective < warp && warp < ids);
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        reopened.source_info(1).unwrap()["transform_kind"],
        "projective"
    );
    assert_eq!(reopened.layer_label(0), 3);
    assert_eq!(reopened.guides.len(), 1);
    assert_eq!(reopened.layer_comps().len(), 1);
    assert_eq!(
        reopened.layers[2].meta.as_deref(),
        Some("opaque newer artwork description")
    );
    reopened.save_native(path.to_str().unwrap()).unwrap();
    assert_eq!(encoded, fs::read(&path).unwrap());
    let cleared = reopened
        .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    cleared.save_native(path.to_str().unwrap()).unwrap();
    let cleared_bytes = fs::read(&path).unwrap();
    assert_eq!(&cleared_bytes[4..12], &[45, 0, 0, 0, 51, 0, 0, 0]);
    assert!(!cleared_bytes.windows(4).any(|w| w == b"WARP"));

    let profile = IccProfile::srgb();
    let master = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            4,
            4,
            (0..16)
                .map(|v| {
                    [
                        v * 97 + 1,
                        v * 101 + 3,
                        65535 - v * 59,
                        if v % 4 == 0 { 0 } else { 32769 },
                    ]
                })
                .collect(),
            profile,
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let typed = typed_document()
        .source_edit(Some(&master), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .geometry(&geometry, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let typed = typed
        .add_adjustment(
            "Invert",
            &json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"})
                .to_string(),
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let typed = typed
        .with_layer_locks(1, doctored_core::doc_lock::LOCK_POSITION)
        .unwrap()
        .unwrap()
        .add_guide(GuideOrientation::Horizontal, 2.0)
        .unwrap();
    let typed = typed.edit_layer_comps(&comp).unwrap().unwrap();
    let typed = typed
        .warp_edit(&edit(field(3, 3, "bicubic")), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    typed
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let encoded = fs::read(&path).unwrap();
    assert_eq!(&encoded[4..16], &[46, 0, 0, 0, 54, 0, 0, 0, 52, 0, 0, 0]);
    let projective = encoded.windows(4).position(|w| w == b"RSPJ").unwrap();
    let warp = encoded.windows(4).position(|w| w == b"WARP").unwrap();
    let ids = encoded.windows(4).position(|w| w == b"LIDS").unwrap();
    assert!(projective < warp && warp < ids);
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.source_info(1).unwrap()["transform_kind"],
        "projective"
    );
    assert_eq!(reopened.guides().len(), 1);
    assert_eq!(reopened.layer_comps().len(), 1);
    assert!(reopened.adjustment_description(2).is_some());
    reopened
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    assert_eq!(encoded, fs::read(&path).unwrap());
    let cleared = reopened
        .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    cleared
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    assert_eq!(
        &fs::read(&path).unwrap()[4..12],
        &[46, 0, 0, 0, 52, 0, 0, 0]
    );
}

#[test]
fn mask_only_group_and_adjustment_survive_native_and_reedit_without_touching_pixels() {
    let dir = tempdir().unwrap();
    let raw: Vec<u8> = (0..11 * 19).map(|i| ((i * 41 + 9) % 256) as u8).collect();
    let mut props = MaskProperties::at((1, -4));
    props.linked = false;
    props.density = 0.64;
    props.feather = 1.5;
    let mut byte = byte_document();
    byte.layers[0].meta = Some(json!({"type":"adjust","op":"invert","params":{}}).to_string());
    byte.layers[0].mask = Some(Arc::new(
        image::GrayImage::from_raw(11, 19, raw.clone()).unwrap(),
    ));
    byte.layers[0].mask_properties = props.clone();
    let original_pixels = byte.layers[0].pixels.clone();
    let mut request = field(3, 3, "bilinear");
    request["target"] = json!("mask");
    let warped = byte
        .warp_edit(&edit(request.clone()), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(LayerRaster::ptr_eq(
        &warped.layers[0].pixels,
        &original_pixels
    ));
    assert_eq!(warped.layers[0].meta, byte.layers[0].meta);
    let path = dir.path().join("adjustment-mask.rz");
    warped.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert!(reopened.warp_info(0).unwrap()["field"].is_object());
    let next = field(4, 2, "bicubic");
    let mut next = next;
    next["target"] = json!("mask");
    let reedited = reopened
        .warp_edit(&edit(next.clone()), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let fresh = byte
        .warp_edit(&edit(next), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        reedited.layers[0].mask.as_ref().unwrap().as_raw(),
        fresh.layers[0].mask.as_ref().unwrap().as_raw()
    );
    let cleared = reopened
        .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(cleared.layers[0].mask.as_ref().unwrap().as_raw(), &raw);
    assert_eq!(cleared.layers[0].mask_properties.offset, (1, -4));

    let typed = typed_document();
    let adjusted = typed
        .add_adjustment(
            "Invert",
            &json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"})
                .to_string(),
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let values: Vec<u16> = (0..11 * 19)
        .map(|i| ((i * 3901 + 179) % 65536) as u16)
        .collect();
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(11, 19, values.clone(), None).unwrap(),
        props,
        false,
    )
    .unwrap();
    let adjusted = adjusted
        .set_layer_mask(1, Some(mask), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let picture_ptr = adjusted.layer_source(0).unwrap().clone();
    let mut request = field(3, 3, "bicubic");
    request["target"] = json!("mask");
    request["layer"] = json!(1);
    let warped = adjusted
        .warp_edit(&edit(request), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(PrecisionImage::ptr_eq(
        warped.layer_source(0).unwrap(),
        &picture_ptr
    ));
    assert!(warped.adjustment_description(1).is_some());
    let path = dir.path().join("typed-adjustment-mask.rz");
    warped
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(reopened.warp_info(1).unwrap()["field"].is_object());
    let cleared = reopened
        .warp_edit(&edit(json!({"op":"clear","layer":1})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        cleared.layer(1).unwrap().mask().unwrap().plane().samples(),
        &values
    );

    let mut grouped = byte_document().group_layers(&[0], "Group").unwrap().0;
    let group = 1;
    grouped.layers[group].mask = Some(Arc::new(image::GrayImage::from_raw(11, 19, raw).unwrap()));
    grouped.layers[group].mask_properties = MaskProperties::at((1, -4));
    let mut request = field(3, 3, "bilinear");
    request["target"] = json!("mask");
    request["layer"] = json!(group);
    let warped = grouped
        .warp_edit(&edit(request), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(warped.warp_info(group).unwrap()["field"].is_object());
    assert!(LayerRaster::ptr_eq(
        &warped.layers[0].pixels,
        &grouped.layers[0].pixels
    ));
}

// ------------------------------------------------ the stored 51/52 files --

/// Byte equality that reports where it breaks: the first differing offset.
fn assert_same_bytes(what: &str, wrote: &[u8], stored: &[u8]) {
    // The stored file predates the rename and starts `RZDC`; this build
    // writes `DRDC`. Every byte after the magic must match.
    assert_eq!(&wrote[..4], b"DRDC", "{what}: this build's magic");
    assert_eq!(&stored[..4], b"RZDC", "{what}: the stored file's magic");
    if wrote[4..] == stored[4..] {
        return;
    }
    let offset = wrote
        .iter()
        .zip(stored)
        .position(|(a, b)| a != b)
        .unwrap_or(wrote.len().min(stored.len()));
    panic!(
        "{what}: wrote {} bytes, the stored file has {} bytes; first mismatch at offset {offset}",
        wrote.len(),
        stored.len()
    );
}

/// The warp-free byte file is pinned against a file the 51 writer
/// (07d9153) wrote, not just against this build's own output.
#[test]
fn a_warp_free_byte_document_rewrites_the_stored_51_file_byte_for_byte() {
    const G: &[u8] = include_bytes!("fixtures/rz_byte_51/warp_fixture.rz");
    let dir = tempdir().unwrap();
    let stored_path = dir.path().join("stored.rz");
    fs::write(&stored_path, G).unwrap();
    let path = dir.path().join("resaved.rz");
    let save = |doc: &DrDocument| -> Vec<u8> {
        doc.save_native(path.to_str().unwrap()).unwrap();
        fs::read(&path).unwrap()
    };

    // (a) The stored file is a 51-writer file: identity word 45 outermost, no
    // word 53, no WARP table.
    assert_eq!(&G[..4], b"RZDC");
    assert_eq!(identity::drdc_word(G, 0), identity::IDENTITY_BYTE);
    assert_ne!(
        identity::drdc_word(G, 1),
        53,
        "the 51 writer never wrote 53"
    );
    assert!(!G.windows(4).any(|w| w == b"WARP"), "and no WARP table");

    // (b) It opens and rewrites to itself.
    let stored = DrDocument::open(stored_path.to_str().unwrap()).expect("the stored file opens");
    assert_same_bytes("reopen then resave", &save(&stored), G);

    // (c) Setting a warp and clearing it writes the stored bytes again.
    let set = stored
        .warp_edit(&edit(field(3, 3, "bilinear")), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let cleared = set
        .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_same_bytes("set then clear", &save(&cleared), G);

    // (d) A freshly built document writes the same file up to its identity
    // wrapper: layer ids come from a process-global counter.
    let fresh = save(&byte_document());
    assert_same_bytes(
        "a fresh document, without its identity wrapper",
        &identity::without_identity(&fresh, 1),
        &identity::without_identity(G, 1),
    );
}

/// The warp-free typed file is pinned against a file the 52 writer
/// (07d9153) wrote, not just against this build's own output.
#[test]
fn a_warp_free_typed_document_rewrites_the_stored_52_file_byte_for_byte() {
    const G: &[u8] = include_bytes!("fixtures/rz_typed_52/warp_fixture.rz");
    let encode = |doc: &PrecisionDocument| doc.encode_native(LIMIT, 0, None).unwrap();

    // (a) The stored file is a 52-writer file: identity word 46 outermost, no
    // word 54, no WARP table.
    assert_eq!(&G[..4], b"RZDC");
    assert_eq!(identity::drdc_word(G, 0), identity::IDENTITY_TYPED);
    assert_ne!(
        identity::drdc_word(G, 1),
        54,
        "the 52 writer never wrote 54"
    );
    assert!(!G.windows(4).any(|w| w == b"WARP"), "and no WARP table");

    // (b) It opens and rewrites to itself.
    let stored = PrecisionDocument::parse_native(G, LIMIT, 0, None).expect("the stored file opens");
    assert_same_bytes("reopen then resave", &encode(&stored), G);

    // (c) Setting a warp and clearing it writes the stored bytes again.
    let set = stored
        .warp_edit(&edit(field(3, 3, "bilinear")), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let cleared = set
        .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_same_bytes("set then clear", &encode(&cleared), G);

    // (d) A freshly built document writes the same file up to its identity
    // wrapper.
    let fresh = encode(&typed_document());
    assert_same_bytes(
        "a fresh document, without its identity wrapper",
        &identity::without_identity(&fresh, 1),
        &identity::without_identity(G, 1),
    );
}

/// `drdc::MAX_DRDC_DECODED_BYTES`, the byte format's decoded cap, restated
/// here rather than read out of the core: DRDC's total layer pixels,
/// `4 × MAX_PIXELS` (100,000,000), at 4 bytes each. The typed format's cap
/// is 8 GiB (`native_file_size.rs`).
const DECODED_CAP: u64 = 1_600_000_000;

/// WARP's decoded admission counts the originals and every serialized
/// fallback plane (RGBA, masks, channels, retained sources) against one cap:
/// at the cap it admits, one byte more in any term refuses, and an overflow
/// refuses.
#[test]
fn serialized_fallback_planes_count_at_the_decoded_cap() {
    use doctored_core::warp_decoded_capacity;
    let original = DECODED_CAP - 10_000;
    assert!(warp_decoded_capacity(original, 4_000, 2_000, 2_000, 2_000).is_ok());
    for extra in [
        (1, 0, 0, 0, 0),
        (0, 1, 0, 0, 0),
        (0, 0, 1, 0, 0),
        (0, 0, 0, 1, 0),
        (0, 0, 0, 0, 1),
    ] {
        assert!(warp_decoded_capacity(
            original + extra.0,
            4_000 + extra.1,
            2_000 + extra.2,
            2_000 + extra.3,
            2_000 + extra.4
        )
        .is_err());
    }
    assert!(warp_decoded_capacity(u64::MAX, 1, 0, 0, 0).is_err());
}
