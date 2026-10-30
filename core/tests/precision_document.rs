//! Independent native framing, true u16 source samples and immutable neutral
//! edits. Corrupt later records must refuse before an earlier PNG is decoded.
use crate::group_roots;
use group_roots::GroupRoots;
use std::io::Cursor;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{ColorType, DynamicImage, ImageBuffer, ImageDecoder, Rgba};

fn image(width: u32, height: u32, profile: &Arc<IccProfile>) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        width,
        height,
        (0..width * height)
            .map(|i| {
                [
                    30000 + (i % 1000) as u16,
                    13001,
                    50002,
                    [0, 1, 10001, 65535][i as usize % 4],
                ]
            })
            .collect(),
        profile.clone(),
        None,
    )
    .unwrap()
}
fn document() -> PrecisionDocument {
    let profile = IccProfile::display_p3();
    let source = image(4, 2, &profile);
    let base = PrecisionDocumentLayer::raster("Base", source.clone(), (0, 0), 0.875, true).unwrap();
    let top =
        PrecisionDocumentLayer::raster("Hidden samples", source.clone(), (-1, 1), 0.625, false)
            .unwrap();
    let inner = PrecisionDocumentLayer::pass_through(
        "Inner",
        vec![PrecisionDocumentLayer::raster("Fine values", source, (1, 0), 0.75, true).unwrap()],
        0.625,
        true,
    )
    .unwrap();
    let group =
        PrecisionDocumentLayer::isolated_normal("Grüpα", vec![top, inner], 0.875, true).unwrap();
    PrecisionDocument::from_parts(
        4,
        2,
        profile,
        Metadata {
            exif: Some(Arc::from(&b"raw EXIF packet"[..])),
            xmp: Some(Arc::from(&b"<xmp/>"[..])),
            iptc: Some(Arc::from(&b"raw IPTC packet"[..])),
        },
        Resolution {
            x: 300.5,
            y: 150.25,
        },
        vec![base, group],
    )
    .unwrap()
}
fn png() -> Vec<u8> {
    let pixels = ImageBuffer::<Rgba<u16>, Vec<u16>>::from_raw(
        2,
        1,
        vec![30000, 12345, 40000, 1, 30001, 12346, 40001, 0],
    )
    .unwrap();
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba16(pixels)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

// Small independently framed v11 fixture: no implementation serializer or
// format-reader helper is used to construct its expected bytes.
fn header(roots: u32, records: u32) -> Vec<u8> {
    let mut bytes = b"RZDC".to_vec();
    for value in [11, 1, 1, 2, 1, roots, records] {
        bytes.extend(value.to_le_bytes());
    }
    let profile = IccProfile::srgb();
    bytes.extend((profile.bytes().len() as u32).to_le_bytes());
    bytes.extend(profile.bytes().iter());
    bytes.extend(72.0_f32.to_le_bytes());
    bytes.extend(72.0_f32.to_le_bytes());
    bytes.extend([0, 0, 0]);
    bytes
}
fn record(kind: u8, png: &[u8], dims: (u32, u32)) -> Vec<u8> {
    let mut bytes = vec![kind, 1, 0, 0];
    bytes.extend(1.0_f32.to_le_bytes());
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(b'L');
    bytes.extend(0_i32.to_le_bytes());
    bytes.extend(0_i32.to_le_bytes());
    bytes.extend(dims.0.to_le_bytes());
    bytes.extend(dims.1.to_le_bytes());
    bytes.extend((png.len() as u64).to_le_bytes());
    bytes.extend(png);
    bytes
}

#[test]
fn native_roundtrip_preserves_sources_groups_context_and_projection() {
    let original = document();
    let projection = original.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    let encoded = original
        .encode_native(REFERENCE_WORKING_BYTES, projection.retained_bytes(), None)
        .unwrap();
    // 46 is 12E-1's always-written identity wrapper, then the body's own
    // version word and header.
    assert_eq!(
        &encoded[..20],
        &[b'D', b'R', b'D', b'C', 46, 0, 0, 0, 13, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]
    );
    let reopened = PrecisionDocument::parse_native(
        &encoded,
        REFERENCE_WORKING_BYTES,
        original.retained_bytes(),
        None,
    )
    .unwrap();
    assert_eq!(reopened.dimensions(), original.dimensions());
    assert_eq!(reopened.profile().bytes(), original.profile().bytes());
    assert_eq!(reopened.metadata(), original.metadata());
    assert_eq!(reopened.resolution(), original.resolution());
    assert_eq!(reopened.layer_count(), 5);
    for index in 0..original.layer_count() {
        let before = original.layer(index).unwrap();
        let after = reopened.layer(index).unwrap();
        assert_eq!(after.name(), before.name());
        assert_eq!(after.opacity(), before.opacity());
        assert_eq!(after.visible(), before.visible());
        assert_eq!(after.offset(), before.offset());
        assert_eq!(after.is_pass_through(), before.is_pass_through());
        assert_eq!(
            after.source().map(|i| i.to_rgba16().ok()),
            before.source().map(|i| i.to_rgba16().ok())
        );
        if let Some(source) = after.source() {
            assert!(Arc::ptr_eq(source.profile(), reopened.profile()));
        }
    }
    assert_eq!(
        reopened
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .to_rgba16()
            .ok(),
        projection.to_rgba16().ok()
    );
    // Inspect the writer's first native PNG through an independent decoder.
    let signature = b"\x89PNG\r\n\x1a\n";
    let offset = encoded
        .windows(signature.len())
        .position(|v| v == signature)
        .unwrap();
    let length = u64::from_le_bytes(encoded[offset - 8..offset].try_into().unwrap()) as usize;
    let decoder =
        image::codecs::png::PngDecoder::new(Cursor::new(&encoded[offset..offset + length]))
            .unwrap();
    assert_eq!(decoder.color_type(), ColorType::Rgba16);
    let DynamicImage::ImageRgba16(raw) = DynamicImage::from_decoder(decoder).unwrap() else {
        panic!("native PNG lost depth")
    };
    assert_eq!(raw.get_pixel(0, 0).0, [30000, 13001, 50002, 0]);
    assert_eq!(raw.get_pixel(1, 0).0, [30001, 13001, 50002, 1]);
}

#[test]
fn independent_native_fixture_keeps_genuine_sub_byte_values() {
    let mut bytes = header(1, 1);
    bytes.extend(record(0, &png(), (2, 1)));
    let decoded =
        PrecisionDocument::parse_native(&bytes, REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert_eq!(
        decoded.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        &[[30000, 12345, 40000, 1], [30001, 12346, 40001, 0]]
    );
    let saved = decoded
        .encode_native(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    let restored =
        PrecisionDocument::parse_native(&saved, REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert_eq!(
        decoded.layer_source(0).unwrap().to_rgba16().ok(),
        restored.layer_source(0).unwrap().to_rgba16().ok()
    );
}

#[test]
fn neutral_edits_share_pixels_preserve_captures_and_detect_noops() {
    let original = document()
        .with_import_warnings(vec!["Imported packet was omitted".into()])
        .unwrap();
    let pointer = original.layer_source(0).unwrap().clone();
    let duplicate = original.duplicate_layer(0).unwrap().unwrap();
    assert_eq!(original.layer_count(), 5);
    assert_eq!(duplicate.layer_count(), 6);
    assert!(PrecisionImage::ptr_eq(
        duplicate.layer_source(0).unwrap(),
        &pointer
    ));
    assert!(PrecisionImage::ptr_eq(
        duplicate.layer_source(1).unwrap(),
        &pointer
    ));
    let changed = original.set_layer_opacity(0, 0.25).unwrap().unwrap();
    assert_eq!(original.layer(0).unwrap().opacity(), 0.875);
    assert_eq!(changed.layer(0).unwrap().opacity(), 0.25);
    assert!(changed.set_layer_opacity(0, 0.25).unwrap().is_none());
    assert!(original.set_layer_opacity(0, f32::NAN).is_err());
    assert!(original.duplicate_layer(100).is_err());
    let grouped = original.group_all(0.75).unwrap().unwrap();
    assert_eq!(grouped.layer_count(), 6);
    assert!(grouped.layer_source(5).is_none());
    assert_eq!(grouped.metadata(), original.metadata());
    let pass = original.group_all_pass_through(1.0).unwrap().unwrap();
    assert!(pass.layer(5).unwrap().is_pass_through());
    assert_eq!(
        pass.render(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .to_rgba16()
            .ok(),
        original
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .to_rgba16()
            .ok()
    );
    let edited_pass = pass.set_layer_opacity(2, 0.125).unwrap().unwrap();
    assert!(edited_pass.layer(5).unwrap().is_pass_through());
    assert_eq!(
        grouped.info().unwrap()["import_warnings"],
        original.info().unwrap()["import_warnings"]
    );
    let reopened = PrecisionDocument::parse_native(
        &grouped
            .encode_native(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap();
    assert_eq!(
        reopened.info().unwrap()["import_warnings"],
        serde_json::json!([])
    );
}

#[test]
fn unknown_tags_bad_framing_and_truncations_refuse() {
    let mut valid = header(1, 1);
    valid.extend(record(0, &png(), (2, 1)));
    for at in [8, 12] {
        let mut bad = valid.clone();
        bad[at] = 99;
        assert!(PrecisionDocument::parse_native(&bad, REFERENCE_WORKING_BYTES, 0, None).is_err());
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(
        PrecisionDocument::parse_native(&trailing, REFERENCE_WORKING_BYTES, 0, None)
            .err()
            .unwrap()
            .contains("trailing")
    );
    for length in [0, 4, 8, 16, 31, header(1, 1).len(), valid.len() - 1] {
        assert!(PrecisionDocument::parse_native(
            &valid[..length],
            REFERENCE_WORKING_BYTES,
            0,
            None
        )
        .is_err());
    }
    let mut wrong_dims = header(1, 1);
    wrong_dims.extend(record(0, &png(), (1, 2)));
    assert!(
        PrecisionDocument::parse_native(&wrong_dims, REFERENCE_WORKING_BYTES, 0, None)
            .err()
            .unwrap()
            .contains("dimensions")
    );
}

#[test]
fn all_records_and_aggregate_planes_are_checked_before_pixel_decoding() {
    let header_only = png()[..33].to_vec(); // No pixel stream: decode must fail.
    let mut bad_later = header(2, 2);
    bad_later.extend(record(0, &header_only, (2, 1)));
    bad_later.extend(record(99, &header_only, (2, 1)));
    let control = DrJobControl::default();
    let error =
        PrecisionDocument::parse_native(&bad_later, REFERENCE_WORKING_BYTES, 0, Some(&control))
            .err()
            .unwrap();
    assert!(
        error.contains("record kind"),
        "must inspect later record before decoding first: {error}"
    );
    assert_eq!(control.scratch_bytes(), 0);
    // `count` records of a `w`×`h` PNG that is its IHDR alone. Valid
    // independent IHDR CRC: the fixture should reach aggregate plane
    // admission, while still containing no IDAT pixels to decode.
    let oversized = |count: u32, w: u32, h: u32| {
        let mut png = header_only.clone();
        png[16..20].copy_from_slice(&w.to_be_bytes());
        png[20..24].copy_from_slice(&h.to_be_bytes());
        let mut crc = u32::MAX;
        for byte in &png[12..29] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320_u32 & (0_u32.wrapping_sub(crc & 1)));
            }
        }
        png[29..33].copy_from_slice(&(!crc).to_be_bytes());
        let mut bytes = header(count, count);
        for _ in 0..count {
            bytes.extend(record(0, &png, (w, h)));
        }
        bytes
    };
    // Eleven 100 MP planes decode to 8.8 GB, past the typed format's 8 GiB
    // decoded cap: the eleventh record refuses.
    let error = PrecisionDocument::parse_native(
        &oversized(11, 10000, 10000),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .err()
    .unwrap();
    assert!(
        error.contains("serialized pixels"),
        "aggregate capacity must precede decode: {error}"
    );
    // The cap to the pixel: ten 10000² planes and one 4608×16003 sum to
    // 2^30 pixels, 8 GiB decoded at 8 bytes a pixel, which the cap admits
    // (the reference limit then refuses); one more row is past it.
    let at_cap = |rows: u32| {
        let mut bytes = header(11, 11);
        for (w, h) in [(10000, 10000); 10].into_iter().chain([(4608, rows)]) {
            let one = oversized(1, w, h);
            bytes.extend(&one[header(1, 1).len()..]);
        }
        PrecisionDocument::parse_native(&bytes, REFERENCE_WORKING_BYTES, 0, None)
            .err()
            .unwrap()
    };
    let pixels: u64 = 10 * 10_000 * 10_000 + 4608 * 16_003;
    assert_eq!(pixels, 1 << 30);
    let error = at_cap(16003);
    assert!(error.contains("working limit"), "{error}");
    let error = at_cap(16004);
    assert!(error.contains("serialized pixels"), "{error}");
    // Nine 24 MP planes (1.728 GB) passed the 1.6 GB cap the format once
    // had. Under today's cap the reader's own admission refuses them at the
    // reference limit, still before any decode.
    let error = PrecisionDocument::parse_native(
        &oversized(9, 6000, 4000),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .err()
    .unwrap();
    assert!(
        error.contains("working limit"),
        "admission must precede decode: {error}"
    );
    // Root/child counts may not reserve multiple copies of the full declared
    // record count while descending through an ultimately invalid tree.
    let mut wrong_children = header(2, 2);
    let mut group = record(1, &[], (1, 1));
    group.truncate(21);
    group.extend(2_u32.to_le_bytes());
    wrong_children.extend(group);
    assert!(
        PrecisionDocument::parse_native(&wrong_children, REFERENCE_WORKING_BYTES, 0, None)
            .err()
            .unwrap()
            .contains("child counts")
    );
}

#[test]
fn a_projection_past_the_old_work_bound_plans_and_native_preservation_holds() {
    // Sixteen nested groups over a 100 MP canvas charge 2 units a pixel each,
    // 3.4·10⁹ with the fill and store: past the 2·10⁹ the render plan used to
    // refuse at. A render runs under a job control, so the plan answers.
    let profile = IccProfile::srgb();
    let mut layer =
        PrecisionDocumentLayer::raster("Source", image(1, 1, &profile), (0, 0), 1.0, true).unwrap();
    for _ in 0..16 {
        layer = PrecisionDocumentLayer::isolated_normal("Group", vec![layer], 1.0, true).unwrap();
    }
    let document = PrecisionDocument::from_parts(
        10000,
        10000,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    let plan = document.render_plan().expect("the render plan answers");
    assert!(plan.work_units > 2_000_000_000, "{}", plan.work_units);
    let info = document.info().unwrap();
    assert!(
        info["render_refusal"].is_null(),
        "{}",
        info["render_refusal"]
    );
    assert_eq!(info["render_scratch_bytes"], plan.scratch_bytes);
    assert_eq!(info["render_output_bytes"], plan.output_bytes);
    assert!(info["native_scratch_bytes"].as_u64().unwrap() > 0);
    let bytes = document
        .encode_native(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    assert_eq!(
        PrecisionDocument::parse_native(&bytes, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .layer_count(),
        17
    );
}

#[test]
fn admission_cancellation_and_failed_save_leave_the_destination_intact() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("document.rz");
    let path = path.to_str().unwrap();
    std::fs::write(path, b"original destination").unwrap();
    let document = document();
    assert!(document.save_native(path, 1, 0, None).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"original destination");
    let cancelled = DrJobControl::default();
    cancelled.cancel();
    assert!(document
        .save_native(path, REFERENCE_WORKING_BYTES, 0, Some(&cancelled))
        .is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"original destination");
    let complete = DrJobControl::default();
    document
        .save_native(path, REFERENCE_WORKING_BYTES, 0, Some(&complete))
        .unwrap();
    assert_eq!(complete.progress(), 1.0);
    assert_eq!(complete.scratch_bytes(), 0);
    let reopened = PrecisionDocument::open_native(path, REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert_eq!(reopened.layer_count(), document.layer_count());
    assert!(PrecisionDocument::open_native(path, 1, 0, None).is_err());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    let estimate = document.native_plan().unwrap();
    // The streamed reader holds the file less its three stored 4×2 PNGs
    // (134 bytes each) plus its 4 MiB window, and the decode scratch
    // charges at least that.
    assert!(estimate.decode_scratch_bytes >= estimate.encoded_bound - 3 * 134 + (4 << 20));
    assert!(estimate.reopen_retained_bytes > estimate.serialized_pixel_bytes);
}

#[test]
fn legacy_byte_files_remain_version_ten_and_reject_current_typed_version() {
    let directory = tempfile::tempdir().unwrap();
    let legacy_path = directory.path().join("legacy.rz");
    let source = image::RgbaImage::from_raw(2, 1, vec![12, 34, 56, 78, 200, 100, 50, 255]).unwrap();
    let legacy = DrDocument::from_pixels(source.clone());
    legacy.save_native(legacy_path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&legacy_path).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 45);
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 10);
    let reopened = DrDocument::open(legacy_path.to_str().unwrap()).unwrap();
    assert_eq!(reopened.flattened().unwrap(), legacy.flattened().unwrap());
    assert!(PrecisionDocument::parse_native(&bytes, REFERENCE_WORKING_BYTES, 0, None).is_err());
    let path = directory.path().join("typed.rz");
    document()
        .save_native(path.to_str().unwrap(), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    // The byte reader refuses a TYPED file by name. Since 12E-1 the word it
    // sees first is the identity wrapper's 46, and the sentence names the
    // reader to use instead.
    assert!(DrDocument::open(path.to_str().unwrap())
        .err()
        .unwrap()
        .contains("use the preserving high-depth document reader"));
}
