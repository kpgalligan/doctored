//! Captured projection export: independent decoder samples, exact validation,
//! unique/additive allocation admission, and atomic refusal through the FFI.

use doctored_core::doc_color::{CARRIES_PROFILE, CARRIES_RESOLUTION, CARRIES_XMP};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_resources_report;
use doctored_core::ffi_precision_export::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_codec::PrecisionFormat;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::ImageDecoder;
use serde_json::Value;
use std::ffi::{c_char, CStr, CString};
use std::io::Cursor;
use std::ptr;
use std::sync::Arc;

unsafe fn text(value: *mut c_char) -> String {
    assert!(!value.is_null());
    let out = unsafe { CStr::from_ptr(value) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(value) };
    out
}

fn fixture(metadata: Metadata) -> (PrecisionDocument, PrecisionImage) {
    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[12345, 23456, 34567, 0], [65535, 1, 257, 2]],
        IccProfile::display_p3(),
        None,
    )
    .unwrap();
    let doc =
        PrecisionDocument::from_image(image.clone(), metadata, Resolution { x: 240., y: 120. })
            .unwrap();
    (doc, image)
}

fn independent_pixels(bytes: &[u8], format: u32) -> (Vec<u16>, Vec<u8>) {
    fn decode(mut decoder: impl ImageDecoder) -> (Vec<u16>, Vec<u8>) {
        assert_eq!(decoder.color_type(), image::ColorType::Rgba16);
        let profile = decoder.icc_profile().unwrap().unwrap_or_default();
        let mut bytes = vec![0; decoder.total_bytes() as usize];
        decoder.read_image(&mut bytes).unwrap();
        (
            bytes
                .chunks_exact(2)
                .map(|p| u16::from_ne_bytes([p[0], p[1]]))
                .collect(),
            profile,
        )
    }
    if format == 0 {
        decode(image::codecs::png::PngDecoder::new(Cursor::new(bytes)).unwrap())
    } else {
        decode(image::codecs::tiff::TiffDecoder::new(Cursor::new(bytes)).unwrap())
    }
}

#[test]
fn supplied_projection_encodes_every_u16_code_and_metadata_without_rendering() {
    let pixels: Vec<_> = (0..=u16::MAX)
        .map(|v| [v, 65535 - v, v.rotate_left(3), v])
        .collect();
    let image =
        PrecisionImage::from_rgba16(65536, 1, pixels.clone(), IccProfile::display_p3(), None)
            .unwrap();
    let xmp: Arc<[u8]> =
        Arc::from(b"<x:xmpmeta xmlns:x='adobe:ns:meta/'>captured</x:xmpmeta>".as_slice());
    let doc = PrecisionDocument::from_image(
        image.clone(),
        Metadata {
            xmp: Some(xmp.clone()),
            ..Metadata::default()
        },
        Resolution { x: 240., y: 120. },
    )
    .unwrap();
    let source_pointer = doc.layer_source(0).unwrap().clone();
    let expected: Vec<_> = pixels.iter().flatten().copied().collect();
    let directory = tempfile::tempdir().unwrap();
    for format in [0, 1] {
        let path = directory.path().join(format!("projection-{format}"));
        let path_c = CString::new(path.to_str().unwrap()).unwrap();
        let mut error = ptr::null_mut();
        let mut report = u32::MAX;
        assert!(
            unsafe {
                dr_precision_doc_save_projection16(
                    &doc,
                    &image,
                    path_c.as_ptr(),
                    format,
                    true,
                    false,
                    REFERENCE_WORKING_BYTES,
                    0,
                    &mut report,
                    std::ptr::null(),
                    &mut error,
                )
            },
            "{}",
            if error.is_null() {
                String::new()
            } else {
                unsafe { text(error) }
            }
        );
        assert!(error.is_null());
        assert_eq!(report, CARRIES_PROFILE | CARRIES_RESOLUTION | CARRIES_XMP);
        let bytes = std::fs::read(&path).unwrap();
        let (decoded, profile) = independent_pixels(&bytes, format);
        assert_eq!(
            decoded, expected,
            "includes hidden RGB and every alpha code"
        );
        assert_eq!(profile, image.profile().bytes().as_ref());
        if format == 0 {
            let mut p = 8;
            let mut found_resolution = false;
            let mut found_xmp = false;
            while p + 12 <= bytes.len() {
                let n = u32::from_be_bytes(bytes[p..p + 4].try_into().unwrap()) as usize;
                let kind = &bytes[p + 4..p + 8];
                let payload = &bytes[p + 8..p + 8 + n];
                if kind == b"pHYs" {
                    assert_eq!(u32::from_be_bytes(payload[..4].try_into().unwrap()), 9449);
                    assert_eq!(u32::from_be_bytes(payload[4..8].try_into().unwrap()), 4724);
                    assert_eq!(payload[8], 1);
                    found_resolution = true;
                }
                if kind == b"iTXt" && payload.starts_with(b"XML:com.adobe.xmp\0") {
                    assert!(payload.ends_with(&xmp));
                    found_xmp = true;
                }
                p += n + 12;
            }
            assert!(found_resolution && found_xmp);
        } else {
            use tiff::tags::Tag;
            let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes)).unwrap();
            assert_eq!(
                decoder.get_tag_u8_vec(Tag::Unknown(700)).unwrap(),
                xmp.as_ref()
            );
            assert_eq!(decoder.get_tag_u16_vec(Tag::ExtraSamples).unwrap(), vec![2]);
            for (tag, expected) in [(Tag::XResolution, 240.), (Tag::YResolution, 120.)] {
                let tiff::decoder::ifd::Value::Rational(numerator, denominator) =
                    decoder.get_tag(tag).unwrap()
                else {
                    panic!("resolution must be a TIFF rational");
                };
                assert_eq!(f64::from(numerator) / f64::from(denominator), expected);
            }
        }
    }
    assert_eq!(image.to_rgba16().ok().unwrap(), pixels);
    assert!(PrecisionImage::ptr_eq(
        doc.layer_source(0).unwrap(),
        &source_pointer
    ));
    assert_eq!(doc.metadata().xmp.as_ref().unwrap(), &xmp);
}

#[test]
fn exact_profile_bytes_storage_and_dimensions_are_required_before_publication() {
    let (doc, source) = fixture(Metadata::default());
    let profile = Arc::new(IccProfile::parse(source.profile().bytes()).unwrap());
    let same =
        PrecisionImage::from_rgba16(2, 1, source.to_rgba16().unwrap(), profile, None).unwrap();
    assert!(
        doc.projection_export_plan(&same).is_ok(),
        "separate owners with equal ICC bytes are valid"
    );
    let wrong_dimensions =
        PrecisionImage::from_rgba16(1, 2, vec![[1; 4]; 2], source.profile().clone(), None).unwrap();
    let float =
        PrecisionImage::from_rgba32(2, 1, vec![[0.5; 4]; 2], source.profile().clone(), None)
            .unwrap();
    let mut different_bytes = source.profile().bytes().to_vec();
    different_bytes[48..52].copy_from_slice(b"TEST"); // Device manufacturer; color model is identical.
    let different_profile = Arc::new(IccProfile::parse(&different_bytes).unwrap());
    assert!(doctored_core::icc::describes_same_space(
        &different_bytes,
        source.profile().bytes()
    ));
    let wrong_profile =
        PrecisionImage::from_rgba16(2, 1, vec![[1; 4]; 2], different_profile, None).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing");
    std::fs::write(&path, b"original destination").unwrap();
    let path_c = CString::new(path.to_str().unwrap()).unwrap();
    for (image, reason) in [
        (&wrong_dimensions, "dimensions"),
        (&float, "storage"),
        (&wrong_profile, "profile bytes"),
    ] {
        let mut error = ptr::null_mut();
        let mut report = 12345;
        unsafe {
            assert!(dr_precision_doc_projection_export_plan(&doc, image, &mut error).is_null());
            assert!(text(error).contains(reason));
            error = ptr::null_mut();
            assert!(!dr_precision_doc_save_projection16(
                &doc,
                image,
                path_c.as_ptr(),
                0,
                true,
                false,
                REFERENCE_WORKING_BYTES,
                0,
                &mut report,
                std::ptr::null(),
                &mut error
            ));
            assert!(text(error).contains(reason));
        }
        assert_eq!(report, 12345);
        assert_eq!(std::fs::read(&path).unwrap(), b"original destination");
    }
}

#[test]
fn unique_input_union_and_codec_additive_alias_excess_have_exact_admission_boundaries() {
    let shared: Arc<[u8]> = Arc::from(vec![42; 128 * 1024]);
    for alias_packets in [false, true] {
        let metadata = if alias_packets {
            Metadata {
                exif: Some(shared.clone()),
                xmp: Some(shared.clone()),
                iptc: Some(shared.clone()),
            }
        } else {
            Metadata::default()
        };
        let (doc, image) = fixture(metadata);
        let mut error = ptr::null_mut();
        let plan: Value = unsafe {
            serde_json::from_str(&text(dr_precision_doc_projection_export_plan(
                &doc, &image, &mut error,
            )))
            .unwrap()
        };
        assert!(error.is_null());
        let documents = [&doc as *const _];
        let images = [&image as *const _];
        let union: Value = unsafe {
            serde_json::from_str(&text(dr_precision_resources_report(
                documents.as_ptr(),
                1,
                images.as_ptr(),
                1,
                &mut error,
            )))
            .unwrap()
        };
        assert!(error.is_null());
        assert_eq!(plan["retained_bytes"], union["unique_retained_bytes"]);
        let p = doc.projection_export_plan(&image).unwrap();
        assert_eq!(
            p.retained_bytes,
            doc.retained_bytes(),
            "shared pixels and ICC are charged once"
        );
        assert_eq!(p.codec_scratch_bytes, 48 * 2 + 16 * 2 + 128 * 1024 * 1024);
        assert_eq!(
            p.host_scratch_bytes,
            p.codec_scratch_bytes + p.codec_additive_overhead_bytes + 1024 * 1024
        );
        assert_eq!(p.codec_additive_overhead_bytes > 0, alias_packets);
        let outer = 417;
        let limit = p.retained_bytes + p.host_scratch_bytes + outer;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output");
        for format in [PrecisionFormat::Png, PrecisionFormat::Tiff] {
            std::fs::write(&path, b"keep").unwrap();
            assert!(doc
                .save_projection16(
                    &image,
                    path.to_str().unwrap(),
                    format,
                    true,
                    true,
                    limit - 1,
                    outer,
                    None,
                )
                .is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"keep");
            doc.save_projection16(
                &image,
                path.to_str().unwrap(),
                format,
                true,
                true,
                limit,
                outer,
                None,
            )
            .unwrap();
        }
        assert!(doc
            .save_projection16(
                &image,
                path.to_str().unwrap(),
                PrecisionFormat::Png,
                true,
                true,
                REFERENCE_WORKING_BYTES,
                u64::MAX,
                None,
            )
            .is_err());
    }
}

#[test]
fn ffi_null_unknown_format_and_filesystem_errors_leave_reports_and_destinations_unchanged() {
    let (doc, image) = fixture(Metadata::default());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing");
    std::fs::write(&path, b"keep").unwrap();
    let path_c = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    unsafe {
        for (document, projection) in [
            (ptr::null(), &image as *const _),
            (&doc as *const _, ptr::null()),
            (ptr::null(), ptr::null()),
        ] {
            assert!(
                dr_precision_doc_projection_export_plan(document, projection, &mut error).is_null()
            );
            assert!(!text(error).is_empty());
            error = ptr::null_mut();
            let mut report = 999;
            assert!(!dr_precision_doc_save_projection16(
                document,
                projection,
                path_c.as_ptr(),
                0,
                true,
                false,
                REFERENCE_WORKING_BYTES,
                0,
                &mut report,
                std::ptr::null(),
                &mut error
            ));
            assert!(!text(error).is_empty());
            error = ptr::null_mut();
            assert_eq!(report, 999);
        }
        assert!(
            dr_precision_doc_projection_export_plan(ptr::null(), ptr::null(), ptr::null_mut())
                .is_null()
        );
        assert!(!dr_precision_doc_save_projection16(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            true,
            false,
            0,
            0,
            ptr::null_mut(),
            std::ptr::null(),
            ptr::null_mut()
        ));
        let missing =
            CString::new(directory.path().join("missing/child").to_str().unwrap()).unwrap();
        for (path, format) in [
            (ptr::null(), 0),
            (path_c.as_ptr(), 2),
            (missing.as_ptr(), 0),
        ] {
            let mut report = 999;
            assert!(!dr_precision_doc_save_projection16(
                &doc,
                &image,
                path,
                format,
                true,
                false,
                REFERENCE_WORKING_BYTES,
                0,
                &mut report,
                std::ptr::null(),
                &mut error
            ));
            assert!(!text(error).is_empty());
            error = ptr::null_mut();
            assert_eq!(report, 999);
        }
    }
    assert_eq!(std::fs::read(path).unwrap(), b"keep");
    assert_eq!(
        std::fs::read_dir(directory.path()).unwrap().count(),
        1,
        "no abandoned temporary export"
    );
}
