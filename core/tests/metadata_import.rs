//! The ImageIO packet bridge must never decode its carrier pixels, adopt its
//! density/profile, or dirty the document for identical metadata.

use doctored_core::ffi::dr_string_free;
use std::ffi::{c_char, CStr};
use std::ptr;

use doctored_core::ffi_color::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_metadata_import::*;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

fn xmp_carrier(packet: &[u8]) -> Vec<u8> {
    // Metadata-only JPEG with no frame/image data: a decoder cannot open it.
    let mut body = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    body.extend_from_slice(packet);
    let mut bytes = vec![0xff, 0xd8, 0xff, 0xe1];
    bytes.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    bytes.extend(body);
    bytes.extend([0xff, 0xd9]);
    bytes
}

#[test]
fn attaches_packets_without_touching_photograph_and_reimport_is_noop() {
    let dir = TempDir::new().unwrap();
    let pixels = RgbaImage::from_pixel(3, 2, Rgba([71, 122, 189, 133]));
    let input = doc_from(&dir, "input.png", &pixels);
    let packet = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><capture>September</capture></x:xmpmeta>";
    let bytes = xmp_carrier(packet);
    unsafe {
        let mut error: *mut c_char = ptr::null_mut();
        let out = dr_doc_import_metadata(
            input,
            bytes.as_ptr(),
            bytes.len(),
            ptr::null_mut(),
            &mut error,
        );
        assert!(!out.is_null(), "{}", take_err_string(error));
        assert_eq!(dr_doc_metadata_len(input, 1), 0);
        assert_eq!(dr_doc_metadata_len(out, 1), packet.len());
        let mut actual = vec![0; packet.len()];
        assert!(dr_doc_metadata(out, 1, actual.as_mut_ptr(), actual.len()));
        assert_eq!(actual, packet);
        assert_eq!(
            *(&*out).layers[0].pixels.contiguous_image(),
            *(&*input).layers[0].pixels.contiguous_image()
        );
        assert_eq!((*out).width, 3);
        assert_eq!((*out).height, 2);
        assert!(std::sync::Arc::ptr_eq(&(*out).profile, &(*input).profile));
        assert_eq!((*out).resolution, (*input).resolution);
        assert!(dr_doc_import_metadata(
            out,
            bytes.as_ptr(),
            bytes.len(),
            ptr::null_mut(),
            &mut error
        )
        .is_null());
        assert!(error.is_null());
        dr_doc_free(out);
        dr_doc_free(input);
    }
}

#[test]
fn bridge_checks_null_and_oversized_inputs_before_reading_memory() {
    let dir = TempDir::new().unwrap();
    let input = doc_from(&dir, "one.png", &RgbaImage::new(1, 1));
    unsafe {
        let mut error: *mut c_char = ptr::null_mut();
        assert!(
            dr_doc_import_metadata(ptr::null(), ptr::null(), 0, ptr::null_mut(), &mut error)
                .is_null()
        );
        assert!(dr_doc_import_metadata(
            input,
            ptr::null(),
            usize::MAX,
            ptr::null_mut(),
            &mut error
        )
        .is_null());
        assert!(error.is_null());
        let byte = 0u8;
        assert!(
            dr_doc_import_metadata(input, &byte, usize::MAX, ptr::null_mut(), &mut error).is_null()
        );
        assert!(take_err_string(error).contains("limit"));
        error = ptr::null_mut();
        assert!(dr_doc_import_metadata(input, &byte, 1, ptr::null_mut(), &mut error).is_null());
        assert!(take_err_string(error).contains("JPEG, PNG or TIFF"));
        dr_doc_free(input);
    }
}

#[test]
fn tiff_capture_omissions_remain_visible_on_noop_attachment() {
    // Tiny hand-built carrier: root EXIF pointer -> a private field (37499,
    // the tag below MakerNote; a MakerNote itself is kept).
    // It has no image payload; packet attachment must not invoke a decoder.
    let bytes = b"II\x2a\0\x08\0\0\0\x01\0\x69\x87\x04\0\x01\0\0\0\x1a\0\0\0\0\0\0\0\x01\0\x7b\x92\x07\0\x04\0\0\0note\0\0\0\0";
    let dir = TempDir::new().unwrap();
    let input = doc_from(&dir, "capture.png", &RgbaImage::new(1, 1));
    unsafe {
        let mut error = ptr::null_mut();
        let mut warnings = ptr::null_mut();
        let out = dr_doc_import_metadata(
            input,
            bytes.as_ptr(),
            bytes.len(),
            &mut warnings,
            &mut error,
        );
        assert!(!out.is_null(), "{}", take_err_string(error));
        assert!(!warnings.is_null());
        let first = CStr::from_ptr(warnings).to_str().unwrap().to_owned();
        assert!(first.contains("TIFF private capture fields were not retained"));
        dr_string_free(warnings);
        warnings = ptr::null_mut();
        assert!(dr_doc_import_metadata(
            out,
            bytes.as_ptr(),
            bytes.len(),
            &mut warnings,
            &mut error
        )
        .is_null());
        assert!(error.is_null());
        assert_eq!(CStr::from_ptr(warnings).to_str().unwrap(), first);
        dr_string_free(warnings);
        dr_doc_free(out);
        dr_doc_free(input);
    }
}

#[test]
fn malformed_tiff_carrier_header_errors_without_mutating_input() {
    let dir = TempDir::new().unwrap();
    let input = doc_from(&dir, "one.png", &RgbaImage::new(1, 1));
    let bytes = b"II\x2a\0";
    unsafe {
        let mut error = ptr::null_mut();
        let mut warnings = ptr::null_mut();
        assert!(dr_doc_import_metadata(
            input,
            bytes.as_ptr(),
            bytes.len(),
            &mut warnings,
            &mut error
        )
        .is_null());
        assert!(take_err_string(error).contains("malformed"));
        assert!(warnings.is_null());
        dr_doc_free(input);
    }
}
