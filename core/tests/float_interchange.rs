//! Phase 11F (I): delivery out of a float document — the exact 32-bit TIFF,
//! the tone-mapped SDR deliverables and the deliberate conversion to u16.
//!
//! The 32-bit export is read back through the 11D `tiff_float` seam and
//! compared sample for sample; the SDR numbers are compared with the map's own
//! formula followed by the sRGB transfer function, both written out here. A
//! 32-bit export with a size mode is compared bit for bit with Image
//! Size's own float resize of the same samples.

use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_export::{output_dimensions, ExportRequest};
use doctored_core::float_document::ToneMap;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::{self, encode_to_vec, FloatTiff};
use serde_json::{json, Value};

use crate::exr_builder::{half_value, nearest_half_bits};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

fn fixture_document() -> PrecisionDocument {
    PrecisionDocument::open_preserving(FIXTURE, LIMIT, 0, None).expect("the float fixture opens")
}

/// The analytic sRGB transfer function, from the specification.
fn srgb_encode(linear: f64) -> f64 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn reinhard(v: f64, white: f64) -> f64 {
    let u = v.max(0.0);
    (u * (1.0 + u / (white * white)) / (1.0 + u)).min(1.0)
}

#[test]
fn a_thirty_two_bit_export_writes_the_samples_exactly() {
    let document = fixture_document();
    let request = ExportRequest::parse("{\"format\":\"tiff\",\"bit_depth\":32}").unwrap();
    let result = document
        .export_encoded(None, &request, LIMIT, 0, None)
        .unwrap();
    assert_eq!(result.info_json()["bit_depth"], 32);
    assert_eq!(result.info_json()["format"], "tiff");
    let decoded = tiff_float::decode(result.bytes(), "export", LIMIT).unwrap();
    assert_eq!((decoded.width, decoded.height), (48, 32));
    assert_eq!(decoded.channels, 4);
    assert_eq!(decoded.alpha, Some(false));
    assert!(decoded.icc.is_some(), "the document profile is embedded");
    assert_eq!(decoded.non_finite, 0);
    let source = document.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    for (i, pixel) in source.iter().enumerate() {
        for (c, value) in pixel.iter().enumerate() {
            assert_eq!(
                value.to_bits(),
                decoded.samples[i * 4 + c].to_bits(),
                "sample {i}:{c}"
            );
        }
    }
    // The exported file reopens as a float document with the same samples.
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("out.tiff");
    std::fs::write(&path, result.bytes()).unwrap();
    let reopened =
        PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(reopened.is_float());
    assert_eq!(
        reopened.layer_source(0).unwrap().to_rgba32().ok().unwrap(),
        source
    );
}

/// The half TIFF. Each sample is the nearest half to the
/// document's f32 (the hand-written search in `common/exr_builder.rs`), the
/// profile is still embedded, and the file reopens as a float document.
#[test]
fn a_half_tiff_export_writes_the_nearest_half_and_reopens() {
    let document = fixture_document();
    let request =
        ExportRequest::parse("{\"format\":\"tiff\",\"bit_depth\":32,\"float_samples\":\"f16\"}")
            .unwrap();
    let result = document
        .export_encoded(None, &request, LIMIT, 0, None)
        .unwrap();
    assert_eq!(result.info_json()["bit_depth"], 32);
    assert_eq!(result.info_json()["format"], "tiff");
    assert_eq!(result.info_json()["float_samples"], "f16");
    let plan = tiff_float::inspect(result.bytes(), "export").unwrap();
    assert_eq!(plan.sample_type, tiff_float::FloatSampleType::F16);
    let decoded = tiff_float::decode(result.bytes(), "export", LIMIT).unwrap();
    assert_eq!(decoded.alpha, Some(false));
    assert!(decoded.icc.is_some(), "the document profile is embedded");
    let source = document.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    for (i, pixel) in source.iter().enumerate() {
        for (c, value) in pixel.iter().enumerate() {
            assert_eq!(
                decoded.samples[i * 4 + c].to_bits(),
                half_value(nearest_half_bits(*value)).to_bits(),
                "sample {i}:{c} = {value}"
            );
        }
    }
    let reopened = reopen(result.bytes());
    assert!(reopened.is_float());
    assert_eq!(reopened.dimensions(), document.dimensions());

    // The default, and the f32 request spelled out, are the same bytes.
    let plain = ExportRequest::parse("{\"format\":\"tiff\",\"bit_depth\":32}").unwrap();
    let spelled =
        ExportRequest::parse("{\"format\":\"tiff\",\"bit_depth\":32,\"float_samples\":\"f32\"}")
            .unwrap();
    let a = document
        .export_encoded(None, &plain, LIMIT, 0, None)
        .unwrap();
    let b = document
        .export_encoded(None, &spelled, LIMIT, 0, None)
        .unwrap();
    assert_eq!(a.bytes(), b.bytes());
    assert_eq!(a.info_json()["float_samples"], "f32");
    assert!(
        result.bytes().len() < a.bytes().len(),
        "half samples are the smaller file"
    );
}

/// The three float containers charge one plan: the 32-bit term does not
/// depend on which of them the request names.
#[test]
fn every_float_container_charges_the_same_plan_and_a_size_mode_resamples_each() {
    let document = fixture_document();
    let scratch = |text: &str| {
        document
            .export_plan(None, &ExportRequest::parse(text).unwrap())
            .unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap()
    };
    let tiff = scratch("{\"format\":\"tiff\",\"bit_depth\":32}");
    for text in [
        "{\"format\":\"exr\",\"bit_depth\":32}",
        "{\"format\":\"exr\",\"bit_depth\":32,\"float_samples\":\"f16\"}",
        "{\"format\":\"hdr\",\"bit_depth\":32}",
        "{\"format\":\"tiff\",\"bit_depth\":32,\"float_samples\":\"f16\"}",
    ] {
        assert_eq!(scratch(text), tiff, "{text}");
    }
    for format in ["exr", "hdr"] {
        let text =
            json!({"format":format,"bit_depth":32,"size":{"mode":"scale","value":0.5}}).to_string();
        let result = document
            .export_encoded(None, &ExportRequest::parse(&text).unwrap(), LIMIT, 0, None)
            .unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(result.info_json()["width"], 24, "{text}");
        assert_eq!(result.info_json()["height"], 16, "{text}");
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join(format!("half.{format}"));
        std::fs::write(&path, result.bytes()).unwrap();
        let reopened =
            PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
        assert!(reopened.is_float(), "{text}");
        assert_eq!(reopened.dimensions(), (24, 16), "{text}");
    }
}

#[test]
fn a_thirty_two_bit_request_refuses_every_stage_but_the_resample_that_would_move_a_sample() {
    for (request, named) in [
        ("{\"format\":\"png\",\"bit_depth\":32}", "tiff, exr or hdr"),
        (
            "{\"format\":\"tiff\",\"bit_depth\":32,\"sharpen\":{\"radius\":1.0,\"amount\":0.5,\"threshold\":0.0}}",
            "sharpen",
        ),
        (
            "{\"format\":\"tiff\",\"bit_depth\":32,\"profile\":\"srgb\"}",
            "profile document",
        ),
        (
            "{\"format\":\"tiff\",\"bit_depth\":32,\"dither\":\"ordered\"}",
            "dithering",
        ),
    ] {
        let error = ExportRequest::parse(request).err().unwrap_or_default();
        assert!(error.contains(named), "{request} -> {error}");
    }
    assert!(ExportRequest::parse("{\"format\":\"tiff\",\"bit_depth\":24\"}").is_err());
}

#[test]
fn a_u16_document_refuses_a_thirty_two_bit_export() {
    let image = doctored_core::precision_image::PrecisionImage::from_rgba16(
        2,
        1,
        vec![[100, 200, 300, 65535]; 2],
        doctored_core::icc::IccProfile::srgb(),
        None,
    )
    .unwrap();
    let document = PrecisionDocument::from_image(
        image,
        doctored_core::metadata::Metadata::default(),
        doctored_core::metadata::Resolution::default(),
    )
    .unwrap();
    let request = ExportRequest::parse("{\"format\":\"tiff\",\"bit_depth\":32}").unwrap();
    let error = document
        .export_encoded(None, &request, LIMIT, 0, None)
        .err()
        .expect("a u16 document has no extended-range samples to write");
    assert!(error.contains("32-bit float document"), "{error}");
}

#[test]
fn an_sdr_export_uses_the_documents_own_tone_map() {
    let document = fixture_document();
    let request = ExportRequest::parse("{\"format\":\"png\",\"bit_depth\":16}").unwrap();
    let clipped = document
        .export_encoded(None, &request, LIMIT, 0, None)
        .unwrap();
    assert_eq!(clipped.info_json()["bit_depth"], 16);
    let mapped = document
        .with_tone_map(ToneMap::Reinhard { white: 4.0 })
        .unwrap()
        .unwrap();
    let reinhard_bytes = mapped
        .export_encoded(None, &request, LIMIT, 0, None)
        .unwrap();
    assert_ne!(
        clipped.bytes(),
        reinhard_bytes.bytes(),
        "the map is what the deliverable differs by"
    );

    // The numbers themselves, through the document's own quantizer.
    let rendered = document.render(LIMIT, 0, None).unwrap();
    assert_eq!(rendered.storage(), PrecisionStorage::Rgba32Float);
    let source = rendered.to_rgba32().ok().unwrap();
    let display = mapped.display_rgba8(&rendered, LIMIT, 0, None).unwrap();
    for (i, pixel) in display.pixels().enumerate() {
        for (c, linear) in source[i][..3].iter().enumerate() {
            let expected = (srgb_encode(reinhard(f64::from(*linear), 4.0)) * 255.0).round() as i32;
            let got = i32::from(pixel.0[c]);
            assert!(
                (got - expected).abs() <= 1,
                "pixel {i}:{c} = {got} vs {expected}"
            );
        }
    }
}

#[test]
fn float_to_u16_carries_the_map_and_stays_linear_light() {
    let document = fixture_document()
        .with_tone_map(ToneMap::Reinhard { white: 4.0 })
        .unwrap()
        .unwrap();
    let converted = document.float_to_u16(LIMIT, 0, None).unwrap();
    assert!(!converted.is_float());
    assert_eq!(converted.tone_map(), None);
    assert_eq!(converted.evaluation(), Evaluation::LinearLight);
    assert_eq!(converted.dimensions(), document.dimensions());
    assert_eq!(converted.layer_count(), 1);
    assert_eq!(
        converted.layer_source(0).unwrap().storage(),
        PrecisionStorage::Rgba16Unorm
    );
    // The first native word is 39, not 40: the result is an ordinary
    // linear-light u16 document that every 11E reader can open.
    // …behind 12E-1's always-written identity wrapper, which is now the
    // outermost word at every depth.
    let bytes = converted.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!(
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        46
    );
    assert_eq!(
        u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        39
    );

    // Its codes are the map followed by the transfer function.
    let rendered = fixture_document().render(LIMIT, 0, None).unwrap();
    let source = rendered.to_rgba32().ok().unwrap();
    let out = converted.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    for (i, pixel) in out.iter().enumerate() {
        for c in 0..3 {
            let expected =
                (srgb_encode(reinhard(f64::from(source[i][c]), 4.0)) * 65535.0).round() as i64;
            let got = i64::from(pixel[c]);
            assert!(
                (got - expected).abs() <= 2,
                "code {i}:{c} = {got} vs {expected}"
            );
        }
        assert_eq!(
            pixel[3],
            (f64::from(source[i][3]) * 65535.0).round() as u16,
            "alpha {i}"
        );
    }
}

#[test]
fn a_float_document_refuses_the_untone_mapped_sixteen_bit_writer() {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("refused.tiff");
    let error = fixture_document()
        .export16(
            path.to_str().unwrap(),
            doctored_core::precision_codec::PrecisionFormat::Tiff,
            true,
            false,
            LIMIT,
            0,
        )
        .expect_err("the untone-mapped writer refuses");
    assert!(error.contains("Export"), "{error}");
    assert!(!path.exists());
}

// ---------------------------------------------------- 32-bit export resize --

/// A 23×17 float layer with negatives, ±65504, partial alpha and alpha-0
/// pixels whose hidden RGB lies outside the visible range, opened through
/// the shipped float TIFF reader.
fn resize_document() -> PrecisionDocument {
    let (w, h) = (23u32, 17u32);
    let samples: Vec<f32> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let r = i.wrapping_mul(0x9E37_79B9) ^ (i >> 3);
            let v = |shift: u32| ((r >> shift) & 0xff) as f32 / 255.0 * 33.0 - 3.0;
            if x < 2 {
                return [60000.0, -60000.0, 60000.0, 0.0];
            }
            if (x, y) == (w / 2, h / 2) {
                return [65504.0, -65504.0, 65504.0, 1.0];
            }
            let alpha = if x == 2 || r % 5 == 0 { 0.4 } else { 1.0 };
            [v(8), v(16), v(24), alpha]
        })
        .collect();
    let tiff = FloatTiff {
        width: w,
        height: h,
        channels: 4,
        alpha: Some(false),
        samples,
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("resize.tiff");
    std::fs::write(&path, encode_to_vec(&tiff, false).unwrap()).unwrap();
    let document =
        PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(document.is_float());
    document
}

/// Writes an export and reopens it through the shipped float reader.
fn reopen(bytes: &[u8]) -> PrecisionDocument {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("export.tiff");
    std::fs::write(&path, bytes).unwrap();
    PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the export reopens")
}

fn bits(pixels: &[[f32; 4]]) -> Vec<u32> {
    pixels.iter().flatten().map(|v| v.to_bits()).collect()
}

/// (a) Every size mode and filter writes exactly what Image Size's float
/// resize of the same samples holds.
#[test]
fn a_thirty_two_bit_size_mode_resamples_like_image_size() {
    let document = resize_document();
    let source = document.layer_source(0).unwrap();
    for size in [
        json!({"mode":"scale","value":0.5}),
        json!({"mode":"long_edge","value":16}),
        json!({"mode":"width","value":9}),
        json!({"mode":"height","value":40}),
    ] {
        for (filter, geometry) in [
            ("lanczos3", "lanczos3"),
            ("catmull-rom", "bicubic"),
            ("nearest", "nearest"),
        ] {
            let text =
                json!({"format":"tiff","bit_depth":32,"size":size,"filter":filter}).to_string();
            let request = ExportRequest::parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
            let (w, h) = output_dimensions(document.dimensions(), &request).unwrap();
            assert_ne!((w, h), document.dimensions(), "{text} resamples");
            let result = document
                .export_encoded(Some(source), &request, LIMIT, 0, None)
                .unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(result.info_json()["bit_depth"], 32);
            assert_eq!(
                (
                    result.info_json()["width"].clone(),
                    result.info_json()["height"].clone()
                ),
                (Value::from(w), Value::from(h)),
                "{text}"
            );
            let reopened = reopen(result.bytes());
            assert!(reopened.is_float(), "{text}");
            assert_eq!(reopened.dimensions(), (w, h), "{text}");
            let resize = json!({"op":"resize","width":w,"height":h,"filter":geometry});
            let resized = document
                .geometry(
                    &GeometryEdit::parse(&resize.to_string()).unwrap(),
                    LIMIT,
                    0,
                    None,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                bits(&reopened.layer_source(0).unwrap().to_rgba32().ok().unwrap()),
                bits(&resized.layer_source(0).unwrap().to_rgba32().ok().unwrap()),
                "{text}: the export equals Image Size's resize"
            );
        }
    }
}

/// (b) The committed scene through the render path: half size, values above
/// 1.0 survive, and every visible sample stays inside the composite's own
/// visible range.
#[test]
fn the_committed_scene_exports_at_half_size_with_its_highlights() {
    if !std::path::Path::new(FIXTURE).exists() {
        eprintln!("skipping: {FIXTURE} is absent");
        return;
    }
    let document = fixture_document();
    let request = ExportRequest::parse(
        "{\"format\":\"tiff\",\"bit_depth\":32,\"size\":{\"mode\":\"scale\",\"value\":0.5}}",
    )
    .unwrap();
    let result = document
        .export_encoded(None, &request, LIMIT, 0, None)
        .unwrap();
    let reopened = reopen(result.bytes());
    assert!(reopened.is_float());
    assert_eq!(reopened.dimensions(), (24, 16));
    let composite = document
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .ok()
        .unwrap();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in composite.iter().filter(|p| p[3] > 0.0) {
        for c in 0..3 {
            min[c] = min[c].min(p[c]);
            max[c] = max[c].max(p[c]);
        }
    }
    let samples = reopened.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    assert!(
        samples
            .iter()
            .any(|p| p[3] > 0.0 && p[..3].iter().any(|&v| v > 1.0)),
        "values above 1.0 survive the resample"
    );
    for (i, p) in samples.iter().enumerate().filter(|(_, p)| p[3] > 0.0) {
        for c in 0..3 {
            assert!(
                p[c] >= min[c] && p[c] <= max[c],
                "sample {i}:{c} = {} leaves [{}, {}]",
                p[c],
                min[c],
                max[c]
            );
        }
    }
}

/// (c) Scale 1 is size none, byte for byte; (d) a resample (an upscale)
/// charges more scratch than the exact copy.
#[test]
fn scale_one_is_size_none_and_a_resample_charges_more() {
    let document = resize_document();
    let source = document.layer_source(0).unwrap();
    let none = ExportRequest::parse("{\"format\":\"tiff\",\"bit_depth\":32}").unwrap();
    let one = ExportRequest::parse(
        "{\"format\":\"tiff\",\"bit_depth\":32,\"size\":{\"mode\":\"scale\",\"value\":1}}",
    )
    .unwrap();
    let a = document
        .export_encoded(Some(source), &none, LIMIT, 0, None)
        .unwrap();
    let b = document
        .export_encoded(Some(source), &one, LIMIT, 0, None)
        .unwrap();
    assert_eq!(a.bytes(), b.bytes(), "scale 1 writes the samples exactly");

    // An upscale, so the output-sized encoder terms cannot hide the
    // resample's own charge.
    let double = ExportRequest::parse(
        "{\"format\":\"tiff\",\"bit_depth\":32,\"size\":{\"mode\":\"scale\",\"value\":2}}",
    )
    .unwrap();
    let scratch = |request: &ExportRequest| {
        document.export_plan(None, request).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap()
    };
    assert!(
        scratch(&double) > scratch(&none),
        "a 32-bit resample charges more than the exact copy: {} vs {}",
        scratch(&double),
        scratch(&none)
    );
}
