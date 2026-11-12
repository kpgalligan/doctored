//! Saved PSD merged transparency is white-matted; raw raster/mask planes are not.
//! Fixtures follow GIMP 3.2.6's independent writer convention. Expected straight
//! samples use a separate rational/f64 inverse, not the product implementation.
use crate::common;
use crate::psd_mask_fixture as fixture;
use common::{cpath, take_err_string};
use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::{dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
use fixture::{fixture, ExpectedMask, Record};
use serde_json::Value;
use std::{ffi::CString, path::Path, ptr};
use tempfile::TempDir;

fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    assert!(error.is_null());
    serde_json::from_str(&take_err_string(raw)).unwrap()
}
fn open(path: &Path, report: &Value, mode: i32) -> Box<DrDocument> {
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            fingerprint.as_ptr(),
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    assert!(error.is_null());
    unsafe { Box::from_raw(raw) }
}
fn save(doc: &DrDocument, path: &Path) -> Vec<u8> {
    let mut error = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, cpath(path).as_ptr(), std::ptr::null(), &mut error) },
        "{}",
        take_err_string(error)
    );
    std::fs::read(path).unwrap()
}
fn reopen(path: &Path) -> Box<DrDocument> {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open(cpath(path).as_ptr(), std::ptr::null(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    unsafe { Box::from_raw(raw) }
}

fn encoded(planes: &[Vec<u8>], width: usize, rle: bool) -> Vec<u8> {
    let mut out = u16::from(rle).to_be_bytes().to_vec();
    if !rle {
        for plane in planes {
            out.extend(plane);
        }
        return out;
    }
    let mut rows = Vec::new();
    for plane in planes {
        for row in plane.chunks_exact(width) {
            let mut packet = vec![128]; // PackBits NOP followed by literal packets
            for chunk in row.chunks(128) {
                packet.push((chunk.len() - 1) as u8);
                packet.extend(chunk);
            }
            out.extend((packet.len() as u16).to_be_bytes());
            rows.push(packet);
        }
    }
    for row in rows {
        out.extend(row);
    }
    out
}

fn psd(
    record: Record,
    canvas: [u32; 2],
    gray: bool,
    merged: &[Vec<u8>],
    alpha: bool,
    rle: bool,
) -> Vec<u8> {
    let mut bytes = fixture(&[record], canvas, gray);
    let base = if gray { 1 } else { 3 };
    // The shared independent fixture has a fixed empty resource section and
    // one positive layer count. Replace only its authored composite framing.
    bytes[12..14].copy_from_slice(&(merged.len() as u16).to_be_bytes());
    assert_eq!(&bytes[42..44], &1i16.to_be_bytes());
    bytes[42..44].copy_from_slice(&(if alpha { -1i16 } else { 1 }).to_be_bytes());
    bytes.truncate(bytes.len() - 2 - canvas[0] as usize * canvas[1] as usize * base);
    bytes.extend(encoded(merged, canvas[0] as usize, rle));
    bytes
}

fn matted(rgba: &[u8], gray: bool) -> Vec<Vec<u8>> {
    let mut channels = vec![Vec::new(); if gray { 2 } else { 4 }];
    for pixel in rgba.chunks_exact(4) {
        let a = u32::from(pixel[3]);
        for c in 0..channels.len() - 1 {
            // Exact GIMP exporter rule, applied only by this fixture writer.
            channels[c].push(((u32::from(pixel[c]) * a + 128) / 255 + 255 - a) as u8);
        }
        channels.last_mut().unwrap().push(pixel[3]);
    }
    channels
}

fn inverse(planes: &[Vec<u8>], gray: bool) -> Vec<u8> {
    let alpha = planes[if gray { 1 } else { 3 }].iter();
    let mut out = Vec::new();
    for (index, &a) in alpha.enumerate() {
        for c in 0..3 {
            let stored = planes[if gray { 0 } else { c }][index];
            let straight = if a == 0 {
                0.
            } else {
                (f64::from(stored) - (255. - f64::from(a))) * 255. / f64::from(a)
            };
            out.push(straight.round().clamp(0., 255.) as u8);
        }
        out.push(a);
    }
    out
}

fn qualify(bytes: &[u8], expected: &[u8], source: &[u8], merged_alpha: bool) {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("merged.psd");
    std::fs::write(&path, bytes).unwrap();
    let notice = report(&path);
    assert_eq!(notice["layered_available"], true, "{notice}");
    assert_eq!(notice["stored_composite_available"], true, "{notice}");
    assert_eq!(
        notice["observations"]
            .to_string()
            .contains("preview stored against white"),
        merged_alpha
    );
    let layers = open(&path, &notice, 0);
    assert_eq!(layers.layers[0].pixels.contiguous_image().as_raw(), source);
    let saved_layers = save(&layers, &temp.path().join("layers.rz"));
    let composite = open(&path, &notice, 1);
    assert_eq!(composite.layers.len(), 1);
    assert_eq!(
        composite.layers[0].pixels.contiguous_image().as_raw(),
        expected
    );
    let native = temp.path().join("composite.rz");
    let saved = save(&composite, &native);
    let again = reopen(&native);
    assert_eq!(again.layers[0].pixels.contiguous_image().as_raw(), expected);
    assert_eq!(save(&again, &temp.path().join("reopened.rz")), saved);
    assert_eq!(
        save(&layers, &temp.path().join("still-layers.rz")),
        saved_layers
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn rgb_raw_and_rle_reconstruct_white_matte_and_keep_auxiliary_channels_separate() {
    // Actual source/white-storage values from the independent GIMP convention;
    // the alpha1 colors cannot be recovered to their original straight values.
    let rgba = [
        211, 87, 13, 0, 63, 137, 201, 1, 101, 211, 67, 127, 197, 31, 173, 255,
    ];
    let mut planes = matted(&rgba, false);
    assert_eq!(
        planes,
        [
            vec![255, 254, 178, 197],
            vec![255, 255, 233, 31],
            vec![255, 255, 161, 173],
            vec![0, 1, 127, 255]
        ]
    );
    let expected = [
        0, 0, 0, 0, 0, 255, 255, 1, 100, 211, 66, 127, 197, 31, 173, 255,
    ];
    assert_eq!(inverse(&planes, false), expected);
    planes.push(vec![33, 77, 129, 211]); // saved auxiliary channel is not transparency
    for rle in [false, true] {
        qualify(
            &psd(
                Record::raster([0, 0, 1, 4], &rgba, false),
                [4, 1],
                false,
                &planes,
                true,
                rle,
            ),
            &expected,
            &rgba,
            true,
        );
    }
}

#[test]
fn grayscale_raw_and_rle_apply_the_same_inverse_after_scalar_alpha_mapping() {
    let rgba = [
        99, 99, 99, 0, 63, 63, 63, 1, 90, 90, 90, 85, 120, 120, 120, 170, 137, 137, 137, 255,
    ];
    let planes = matted(&rgba, true);
    let expected = [
        0, 0, 0, 0, 0, 0, 0, 1, 90, 90, 90, 85, 120, 120, 120, 170, 137, 137, 137, 255,
    ];
    assert_eq!(inverse(&planes, true), expected);
    for rle in [false, true] {
        qualify(
            &psd(
                Record::raster([0, 0, 1, 5], &rgba, true),
                [5, 1],
                true,
                &planes,
                true,
                rle,
            ),
            &expected,
            &rgba,
            true,
        );
    }
}

#[test]
fn positive_layer_count_never_treats_auxiliary_coverage_as_merged_transparency() {
    let rgba = [31, 77, 193, 17, 43, 101, 169, 255];
    let planes = [
        vec![254, 178],
        vec![255, 233],
        vec![255, 161],
        vec![1, 127],
        vec![19, 73],
    ];
    let expected = [254, 255, 255, 255, 178, 233, 161, 255];
    for rle in [false, true] {
        qualify(
            &psd(
                Record::raster([0, 0, 1, 2], &rgba, false),
                [2, 1],
                false,
                &planes,
                false,
                rle,
            ),
            &expected,
            &rgba,
            false,
        );
    }
}

#[test]
fn inverse_has_explicit_zero_opaque_saturation_and_nearest_tie_behavior() {
    let rgba = [31, 77, 193, 17].repeat(4);
    let planes = [
        vec![12, 0, 128, 7],
        vec![34, 254, 128, 83],
        vec![56, 255, 128, 231],
        vec![0, 1, 254, 255],
    ];
    // Alpha254/stored128 is exactly127.5: the declared native-byte policy rounds up.
    let expected = [
        0, 0, 0, 0, 0, 0, 255, 1, 128, 128, 128, 254, 7, 83, 231, 255,
    ];
    assert_eq!(inverse(&planes, false), expected);
    for rle in [false, true] {
        qualify(
            &psd(
                Record::raster([0, 0, 1, 4], &rgba, false),
                [4, 1],
                false,
                &planes,
                true,
                rle,
            ),
            &expected,
            &rgba,
            true,
        );
    }
}

#[test]
fn exhaustive_alpha_color_pairs_keep_exact_inverse_and_bounded_opaque_backdrop_appearance() {
    let mut rgba = Vec::with_capacity(256 * 256 * 4);
    for a in 0..=255u8 {
        for c in 0..=255u8 {
            rgba.extend([
                c,
                255 - c,
                c.wrapping_mul(17).wrapping_add(a.wrapping_mul(11)),
                a,
            ]);
        }
    }
    let planes = matted(&rgba, false);
    let expected = inverse(&planes, false);
    let mut weighted_max = 0f64;
    let mut backdrop_max = 0f64;
    for (source, decoded) in rgba.chunks_exact(4).zip(expected.chunks_exact(4)) {
        assert_eq!(source[3], decoded[3]);
        for c in 0..3 {
            let alpha = f64::from(source[3]) / 255.;
            let difference = f64::from(source[c].abs_diff(decoded[c])) * alpha;
            weighted_max = weighted_max.max(difference);
            for backdrop in [0., 255.] {
                let original = (f64::from(source[c]) * alpha + backdrop * (1. - alpha)).round();
                let reconstructed =
                    (f64::from(decoded[c]) * alpha + backdrop * (1. - alpha)).round();
                backdrop_max = backdrop_max.max((original - reconstructed).abs());
            }
        }
    }
    assert!(weighted_max <= 254. / 255. + 1e-12, "{weighted_max}");
    assert!(backdrop_max <= 2., "fixed appearance gate: {backdrop_max}");
    assert!(weighted_max > 0.99 && backdrop_max >= 1.);
    println!(
        "all65536 alpha/color pairs: weighted max{weighted_max}; black/white max{backdrop_max}"
    );
    for rle in [false, true] {
        qualify(
            &psd(
                Record::raster([0, 0, 256, 256], &rgba, false),
                [256, 256],
                false,
                &planes,
                true,
                rle,
            ),
            &expected,
            &rgba,
            true,
        );
    }
}

#[test]
fn composite_reconstruction_never_changes_independent_raster_or_rle_mask_storage() {
    let rgba = [
        31, 77, 193, 17, 43, 101, 169, 255, 211, 87, 13, 0, 101, 211, 67, 127,
    ];
    let mask = ExpectedMask {
        bounds: [0, 0, 2, 2],
        coverage: vec![0, 73, 173, 255],
        outside: 255,
        enabled: true,
    };
    let record = Record::raster([0, 0, 2, 2], &rgba, false).mask(&mask, true);
    let planes = matted(&rgba, false);
    let bytes = psd(record, [2, 2], false, &planes, true, true);
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("masked.psd");
    std::fs::write(&path, &bytes).unwrap();
    let notice = report(&path);
    let layers = open(&path, &notice, 0);
    assert_eq!(layers.layers[0].pixels.contiguous_image().as_raw(), &rgba);
    assert_eq!(
        layers.layers[0].mask.as_ref().unwrap().as_raw(),
        &mask.coverage
    );
    assert_eq!(layers.layers[0].mask_properties.outside_coverage, 255);
    let before = save(&layers, &temp.path().join("before.rz"));
    let composite = open(&path, &notice, 1);
    assert_eq!(
        composite.layers[0].pixels.contiguous_image().as_raw(),
        &inverse(&planes, false)
    );
    assert_eq!(save(&layers, &temp.path().join("after.rz")), before);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
