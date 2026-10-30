//! The one-image cap a document keeps (`dr_single_image_capacity`),
//! asked by the host before it decodes a still and by the 16-bit file open
//! before its scratch admission. The native PNG record limit
//! (`MAX_DRDC_PNG_LEN`) is 1 GiB and a 16-bit plane is no longer held to the
//! `image` decoder's 512 MiB allocation cap (it never passes through that
//! decoder), so one image at either depth fits whenever its extent is within
//! `MAX_PIXELS` (100,000,000). Every oracle is a literal; the sentence is
//! written out independently of the core.
use crate::common::take_err_string;

use doctored_core::ffi_host_pixels::dr_single_image_capacity;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

use std::ffi::{c_char, CString};
use std::ptr;

/// `pixel_count`'s sentence, the refusal of every extent past `MAX_PIXELS`.
const PAST_THE_PIXEL_LIMIT: &str =
    "precision prototype dimensions exceed the supported pixel limit";

fn capacity(w: u32, h: u32, depth: u32) -> Result<(), String> {
    let mut err: *mut c_char = ptr::null_mut();
    if unsafe { dr_single_image_capacity(w, h, depth, &mut err) } {
        assert!(err.is_null());
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

#[test]
fn images_inside_the_cap_pass() {
    // 12500×8000 and 10000² are 100 MP, the document limit, at 16 bits (800
    // MB of samples, a native PNG record of about 800 MB); 8192² is the first
    // square the old 512 MiB record limit refused; 12500×8000 fits at 8 bits.
    for (w, h, depth) in [
        (12500, 8000, 16),
        (10000, 10000, 16),
        (8192, 8192, 16),
        (9456, 7096, 16),
        (12500, 8000, 8),
    ] {
        assert_eq!(capacity(w, h, depth), Ok(()), "{w}×{h}@{depth}");
    }
}

#[test]
fn an_extent_past_the_pixel_limit_refuses() {
    // 12500 × 8001 = 100,012,500 and 10001 × 10000 = 100,010,000 pixels.
    for (w, h, depth) in [(12500, 8001, 16), (10001, 10000, 16), (10001, 10000, 8)] {
        assert_eq!(
            capacity(w, h, depth),
            Err(PAST_THE_PIXEL_LIMIT.into()),
            "{w}×{h}@{depth}"
        );
    }
}

#[test]
fn only_depths_8_and_16_are_accepted() {
    // 264 and 272 would truncate to 8 and 16 through `as u8`.
    for depth in [10, 264, 272] {
        assert_eq!(
            capacity(16, 16, depth),
            Err("unsupported native source depth".into()),
            "depth {depth}"
        );
    }
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut result = (data.len() as u32).to_be_bytes().to_vec();
    result.extend(kind);
    result.extend(data);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&result[4..]);
    result.extend(hasher.finalize().to_be_bytes());
    result
}

#[test]
fn a_16_bit_png_past_the_pixel_limit_refuses_before_its_decode() {
    // An untagged RGBA16 PNG declaring 12500×8001 with a tiny IDAT: only the
    // header is read before the limit refuses, so no 800 MB decode is tried.
    let mut ihdr = Vec::new();
    ihdr.extend(12500_u32.to_be_bytes());
    ihdr.extend(8001_u32.to_be_bytes());
    ihdr.extend([16, 6, 0, 0, 0]);
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(chunk(b"IHDR", &ihdr));
    // An empty stored zlib stream: header, one final empty block, Adler-32 of nothing.
    bytes.extend(chunk(
        b"IDAT",
        &[0x78, 0x01, 1, 0, 0, 0xff, 0xff, 0, 0, 0, 1],
    ));
    bytes.extend(chunk(b"IEND", &[]));
    let path = std::env::temp_dir().join(format!("rz-614-{}.png", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_open(
            c_path.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let _ = std::fs::remove_file(&path);
    if !document.is_null() {
        unsafe { dr_precision_doc_free(document) };
        panic!("a 12500×8001 RGBA16 PNG opened");
    }
    assert_eq!(take_err_string(err), PAST_THE_PIXEL_LIMIT);
}
