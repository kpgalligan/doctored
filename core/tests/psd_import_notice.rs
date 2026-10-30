//! PSD compatibility reports and explicit saved-composite import, exercised
//! through C APIs with independently constructed raw/RLE PSD streams.
use crate::common;
use common::{cpath, take_err_string};
use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
use doctored_core::icc::IccProfile;
use image::Rgba;
use serde_json::Value;
use std::ffi::CString;
use std::ptr;
use tempfile::TempDir;

fn resource(id: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"8BIM".to_vec();
    bytes.extend(id.to_be_bytes());
    bytes.extend([0, 0]); // empty even-padded Pascal name
    bytes.extend((body.len() as u32).to_be_bytes());
    bytes.extend(body);
    if !body.len().is_multiple_of(2) {
        bytes.push(0);
    }
    bytes
}

fn layer(mask: bool, offcanvas: bool) -> Vec<u8> {
    let mut record = Vec::new();
    for value in [
        0i32,
        if offcanvas { -1 } else { 0 },
        1,
        if offcanvas { 1 } else { 2 },
    ] {
        record.extend(value.to_be_bytes());
    }
    let channels: &[i16] = if mask {
        &[0, 1, 2, -1, -2]
    } else {
        &[0, 1, 2, -1]
    };
    record.extend((channels.len() as u16).to_be_bytes());
    for id in channels {
        record.extend(id.to_be_bytes());
        record.extend(4u32.to_be_bytes());
    }
    record.extend(b"8BIMnorm");
    record.extend([255, 0, 8, 0]);
    let extra = [0, 0, 0, 0, 0, 0, 0, 0, 3, b'A', b'r', b't'];
    record.extend((extra.len() as u32).to_be_bytes());
    record.extend(extra);
    for id in channels {
        record.extend([0, 0]);
        record.extend(if *id == -1 { [255, 255] } else { [200, 210] });
    }
    record
}

#[allow(clippy::too_many_arguments)]
fn psd(
    depth: u16,
    color: u16,
    planes: &[&[u8]],
    mask: bool,
    offcanvas: bool,
    alpha: bool,
    rle: bool,
    resources: &[u8],
) -> Vec<u8> {
    let mut output = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    output.extend((planes.len() as u16).to_be_bytes());
    output.extend(1u32.to_be_bytes());
    output.extend(2u32.to_be_bytes());
    output.extend(depth.to_be_bytes());
    output.extend(color.to_be_bytes());
    output.extend(0u32.to_be_bytes());
    output.extend((resources.len() as u32).to_be_bytes());
    output.extend(resources);
    let mut info = (if alpha { -1i16 } else { 1 }).to_be_bytes().to_vec();
    info.extend(layer(mask, offcanvas));
    if info.len() % 2 != 0 {
        info.push(0);
    }
    let mut layers = (info.len() as u32).to_be_bytes().to_vec();
    layers.extend(info);
    layers.extend(0u32.to_be_bytes());
    output.extend((layers.len() as u32).to_be_bytes());
    output.extend(layers);
    output.extend(u16::from(rle).to_be_bytes());
    if rle {
        for plane in planes {
            output.extend(((plane.len() + 1) as u16).to_be_bytes());
        }
    }
    for plane in planes {
        if rle {
            output.push((plane.len() - 1) as u8);
        }
        output.extend(*plane);
    }
    output
}

fn report(path: &std::path::Path) -> Value {
    let mut error = ptr::null_mut();
    let result = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!result.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(result)).unwrap()
}
fn open(path: &std::path::Path, mode: i32, report: &Value) -> Result<Box<DrDocument>, String> {
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let result =
        unsafe { dr_doc_open_psd(cpath(path).as_ptr(), mode, fingerprint.as_ptr(), &mut error) };
    if result.is_null() {
        Err(take_err_string(error))
    } else {
        Ok(unsafe { Box::from_raw(result) })
    }
}

#[test]
fn parser_observed_masks_and_bounds_are_separate_from_general_limits() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("mask.psd");
    let bytes = psd(
        8,
        3,
        &[&[10, 11], &[20, 21], &[30, 31]],
        true,
        true,
        false,
        false,
        &[],
    );
    std::fs::write(&path, bytes).unwrap();
    let r = report(&path);
    let losses = r["verified_losses"].to_string();
    assert!(losses.contains("raster mask") && !losses.contains("outside the canvas"));
    assert!(r["observations"]
        .to_string()
        .contains("complete raster rectangle"));
    assert!(
        !losses.contains("styles"),
        "discarded records were not verified"
    );
    assert!(r["general_limitations"]
        .to_string()
        .contains("Layer styles"));
    assert_eq!(r["stored_composite_available"], true);
}

#[test]
fn explicit_composite_retains_saved_pixels_and_rgb_profile_and_ppi() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("p3.psd");
    let profile = IccProfile::display_p3();
    let mut resources = resource(1039, profile.bytes());
    let mut density = Vec::new();
    for ppi in [300u32, 240] {
        density.extend((ppi << 16).to_be_bytes());
        density.extend([0, 1, 0, 1]);
    }
    resources.extend(resource(1005, &density));
    std::fs::write(
        &path,
        psd(
            8,
            3,
            &[&[10, 11], &[20, 21], &[30, 31]],
            false,
            false,
            false,
            false,
            &resources,
        ),
    )
    .unwrap();
    let r = report(&path);
    let flattened = open(&path, 1, &r).unwrap();
    let layered = open(&path, 0, &r).unwrap();
    assert_eq!(
        flattened.layers[0]
            .pixels
            .contiguous_image()
            .get_pixel(0, 0),
        &Rgba([10, 20, 30, 255])
    );
    assert_eq!(
        layered.layers[0].pixels.contiguous_image().get_pixel(0, 0),
        &Rgba([200, 200, 200, 255])
    );
    for doc in [flattened, layered] {
        assert_eq!(doc.profile.bytes(), profile.bytes());
        assert_eq!((doc.resolution.x, doc.resolution.y), (300.0, 240.0));
    }
}

#[test]
fn composite_raw_and_rle_distinguish_transparency_from_saved_alpha() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("alpha.psd");
    for rle in [false, true] {
        for alpha in [false, true] {
            // Historical fixtures incorrectly treated merged transparency
            // colors as straight. These declared bytes instead encode a white
            // matte for source RGB [10,20,30]/[11,21,31] at alpha80/90.
            // Their exact inverse is quantized; ordinary extra saved channels
            // still leave the original opaque RGB codes untouched.
            let colors = if alpha {
                [[178, 169], [181, 172], [184, 176]]
            } else {
                [[10, 11], [20, 21], [30, 31]]
            };
            std::fs::write(
                &path,
                psd(
                    8,
                    3,
                    &[&colors[0], &colors[1], &colors[2], &[80, 90], &[22, 33]],
                    false,
                    false,
                    alpha,
                    rle,
                    &[],
                ),
            )
            .unwrap();
            let r = report(&path);
            let doc = open(&path, 1, &r).unwrap();
            assert_eq!(
                doc.layers[0].pixels.contiguous_image().get_pixel(0, 0),
                &Rgba(if alpha {
                    [10, 19, 29, 80]
                } else {
                    [10, 20, 30, 255]
                })
            );
            assert!(r["verified_losses"].to_string().contains("saved channels"));
        }
    }
}

#[test]
fn grayscale_composite_alpha_is_coverage_not_a_green_color_plane() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("gray.psd");
    for rle in [false, true] {
        std::fs::write(
            &path,
            psd(
                8,
                1,
                // White-matted gray90 at alpha85 and gray120 at alpha170;
                // these thirds are exactly representable in both directions.
                &[&[200, 165], &[85, 170]],
                false,
                false,
                true,
                rle,
                &[],
            ),
        )
        .unwrap();
        let r = report(&path);
        let doc = open(&path, 1, &r).unwrap();
        assert_eq!(
            doc.layers[0].pixels.contiguous_image().get_pixel(0, 0),
            &Rgba([90, 90, 90, 85])
        );
    }
}

#[test]
fn unsupported_depth_color_and_profile_never_fall_back() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("unsupported.psd");
    for (depth, color) in [(16, 3), (32, 3), (8, 4), (8, 9)] {
        std::fs::write(
            &path,
            psd(
                depth,
                color,
                &[&[10, 11], &[20, 21], &[30, 31]],
                false,
                false,
                false,
                false,
                &[],
            ),
        )
        .unwrap();
        let r = report(&path);
        // #547 Lab phase 3a: a Lab file's report is the Lab open's, which
        // opens its stored composite either way; the RGB opens below still
        // refuse it.
        assert_eq!(r["import_precision"] == "lab", color == 9, "{r}");
        assert_eq!(r["layered_available"], color == 9, "{r}");
        assert_eq!(r["stored_composite_available"], color == 9, "{r}");
        assert!(open(&path, 0, &r).is_err());
        assert!(open(&path, 1, &r).is_err());
    }
    let bad_profile = resource(1039, b"not an ICC");
    std::fs::write(
        &path,
        psd(
            8,
            3,
            &[&[10, 11], &[20, 21], &[30, 31]],
            false,
            false,
            false,
            false,
            &bad_profile,
        ),
    )
    .unwrap();
    let r = report(&path);
    assert_eq!(r["stored_composite_available"], false);
    assert!(r["stored_composite_refusal"]
        .as_str()
        .unwrap()
        .contains("ICC"));
}

#[test]
fn stale_preflight_null_arguments_and_bad_composite_refuse_without_panics() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("stale.psd");
    let mut bytes = psd(
        8,
        3,
        &[&[10, 11], &[20, 21], &[30, 31]],
        false,
        false,
        false,
        false,
        &[],
    );
    std::fs::write(&path, &bytes).unwrap();
    let before = report(&path);
    bytes.pop();
    std::fs::write(&path, &bytes).unwrap();
    assert!(open(&path, 1, &before)
        .err()
        .unwrap()
        .contains("changed since"));
    let after = report(&path);
    assert_eq!(after["stored_composite_available"], false);
    unsafe {
        let mut error = ptr::null_mut();
        assert!(dr_psd_import_report(ptr::null(), &mut error).is_null());
        assert!(take_err_string(error).contains("NULL"));
        error = ptr::null_mut();
        assert!(dr_doc_open_psd(ptr::null(), 1, ptr::null(), &mut error).is_null());
        assert!(take_err_string(error).contains("NULL"));
        error = ptr::null_mut();
        assert!(dr_doc_open_psd(cpath(&path).as_ptr(), 1, ptr::null(), &mut error).is_null());
        assert!(take_err_string(error).contains("NULL"));
        dr_doc_free(ptr::null_mut());
    }
}

#[test]
fn unusable_layer_records_still_offer_the_independent_saved_composite() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("broken-layer.psd");
    let mut bytes = psd(
        8,
        3,
        &[&[10, 11], &[20, 21], &[30, 31]],
        false,
        false,
        false,
        false,
        &[],
    );
    let blend = bytes.windows(8).position(|b| b == b"8BIMnorm").unwrap() + 4;
    bytes[blend..blend + 4].copy_from_slice(b"nope");
    std::fs::write(&path, &bytes).unwrap();
    let r = report(&path);
    assert_eq!(r["layered_available"], false);
    assert_eq!(r["stored_composite_available"], true);
    assert!(open(&path, 0, &r).is_err());
    let composite = open(&path, 1, &r).unwrap();
    assert_eq!(
        composite.layers[0]
            .pixels
            .contiguous_image()
            .get_pixel(0, 0),
        &Rgba([10, 20, 30, 255])
    );
}

#[test]
fn no_layer_records_and_invalid_composite_offer_neither_mode() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("no-image.psd");
    let mut bytes = psd(
        8,
        3,
        &[&[10, 11], &[20, 21], &[30, 31]],
        false,
        false,
        false,
        false,
        &[],
    );
    // Header + empty color/resources are 34 bytes. Replace the complete
    // layer section with a zero length, then truncate the raw composite.
    let layer_len = u32::from_be_bytes(bytes[34..38].try_into().unwrap()) as usize;
    bytes.splice(34..38 + layer_len, [0, 0, 0, 0]);
    bytes.pop();
    std::fs::write(&path, &bytes).unwrap();
    let r = report(&path);
    assert_eq!(r["layered_available"], false);
    assert_eq!(r["stored_composite_available"], false);
}
