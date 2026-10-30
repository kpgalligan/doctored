//! Image ▸ Trim's kept rectangle through the C FFI, at 8 bits
//! (`dr_image_trim_rect`) and at 16 bits and float
//! (`dr_precision_image_trim_rect`). The oracle is this file's own scan:
//! every pixel tested against the basis one at a time, the bounding box of
//! the kept ones, unchecked edges put back at the image edge.

use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_trim::{dr_image_trim_rect, dr_precision_image_trim_rect};
use doctored_core::icc::IccProfile;
use doctored_core::precision_image::PrecisionImage;

const W: u32 = 13;
const H: u32 = 9;

/// A picture with a transparent margin on the left and top, a white margin
/// on the right and bottom, and content inside.
fn picture() -> Vec<[u8; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if x < 2 || y < 1 {
                [0, 0, 0, 0]
            } else if x > 9 || y > 6 {
                [255, 255, 255, 255]
            } else if (x + y) % 3 == 0 {
                [30, 60, 90, 255]
            } else {
                [255, 255, 255, 255]
            }
        })
        .collect()
}

/// `px` with its transparent margin painted white.
fn opaque(px: &[[u8; 4]]) -> Vec<[u8; 4]> {
    px.iter()
        .map(|p| if p[3] == 0 { [255; 4] } else { *p })
        .collect()
}

/// The oracle.
fn expected<T: PartialEq + Copy>(
    px: &[[T; 4]],
    alpha_zero: impl Fn([T; 4]) -> bool,
    basis: u32,
    edges: u32,
) -> Option<[u32; 4]> {
    let corner = match basis {
        0 => None,
        1 => Some(px[0]),
        2 => Some(px[px.len() - 1]),
        _ => return None,
    };
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, p) in px.iter().enumerate() {
        let trimmable = match corner {
            None => alpha_zero(*p),
            Some(c) => *p == c,
        };
        if !trimmable {
            let (x, y) = (i as u32 % W, i as u32 / W);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 == u32::MAX {
        return None;
    }
    let top = if edges & 1 != 0 { y0 } else { 0 };
    let bottom = if edges & 2 != 0 { y1 } else { H - 1 };
    let left = if edges & 4 != 0 { x0 } else { 0 };
    let right = if edges & 8 != 0 { x1 } else { W - 1 };
    Some([left, top, right - left + 1, bottom - top + 1])
}

fn byte_rect(px: &[[u8; 4]], basis: i32, edges: u32) -> Option<[u32; 4]> {
    let bytes: Vec<u8> = px.iter().flatten().copied().collect();
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), W, H) };
    assert!(!img.is_null());
    let mut rect = [77u32; 4];
    let ok = unsafe { dr_image_trim_rect(img, basis, edges, rect.as_mut_ptr()) };
    unsafe { dr_image_free(img) };
    if ok {
        Some(rect)
    } else {
        assert_eq!(rect, [77; 4], "a refusal leaves rect_out untouched");
        None
    }
}

fn typed_rect(img: &PrecisionImage, basis: i32, edges: u32) -> Option<[u32; 4]> {
    let mut rect = [77u32; 4];
    let ok = unsafe { dr_precision_image_trim_rect(img, basis, edges, rect.as_mut_ptr()) };
    if ok {
        Some(rect)
    } else {
        assert_eq!(rect, [77; 4], "a refusal leaves rect_out untouched");
        None
    }
}

#[test]
fn byte_trim_matches_the_scan_for_every_basis_and_edge_mask() {
    let px = picture();
    for basis in 0..3u32 {
        for edges in 0..16u32 {
            assert_eq!(
                byte_rect(&px, basis as i32, edges),
                expected(&px, |p| p[3] == 0, basis, edges),
                "basis {basis} edges {edges}"
            );
        }
    }
    // Spot values the scan must give: transparent trims to x 2.., y 1..;
    // with the transparent margin painted white, bottom-right (white) trims
    // to the content box.
    assert_eq!(byte_rect(&px, 0, 15), Some([2, 1, 11, 8]));
    assert_eq!(byte_rect(&opaque(&px), 2, 15), Some([2, 1, 8, 6]));
    assert_eq!(byte_rect(&opaque(&px), 1, 15), Some([2, 1, 8, 6]));
}

#[test]
fn byte_trim_refuses_an_all_trimmable_image_an_unknown_basis_and_null() {
    let clear = vec![[0u8, 0, 0, 0]; (W * H) as usize];
    assert_eq!(byte_rect(&clear, 0, 15), None);
    assert_eq!(byte_rect(&clear, 1, 15), None);
    let px = picture();
    assert_eq!(byte_rect(&px, 3, 15), None);
    assert_eq!(byte_rect(&px, -1, 15), None);
    let mut rect = [0u32; 4];
    unsafe {
        assert!(!dr_image_trim_rect(ptr::null(), 0, 15, rect.as_mut_ptr()));
        assert!(!dr_precision_image_trim_rect(
            ptr::null(),
            0,
            15,
            rect.as_mut_ptr()
        ));
        let img = dr_image_from_rgba(
            px.iter().flatten().copied().collect::<Vec<_>>().as_ptr(),
            W,
            H,
        );
        assert!(!dr_image_trim_rect(img, 0, 15, ptr::null_mut()));
        dr_image_free(img);
    }
}

#[test]
fn sixteen_bit_trim_compares_exact_codes() {
    // One code of difference in the white margin is content at 16 bits.
    let mut px: Vec<[u16; 4]> = opaque(&picture())
        .iter()
        .map(|p| p.map(|v| u16::from(v) * 257))
        .collect();
    px[(5 * W + 12) as usize] = [65535, 65534, 65535, 65535];
    let img = PrecisionImage::from_rgba16(W, H, px.clone(), IccProfile::srgb(), None).unwrap();
    for basis in 0..3u32 {
        for edges in [0, 1, 6, 9, 15] {
            assert_eq!(
                typed_rect(&img, basis as i32, edges),
                expected(&px, |p| p[3] == 0, basis, edges),
                "basis {basis} edges {edges}"
            );
        }
    }
    assert_eq!(typed_rect(&img, 2, 15), Some([2, 1, 11, 6]));
    assert_eq!(typed_rect(&img, 7, 15), None);
}

#[test]
fn float_trim_compares_bit_patterns() {
    let mut px: Vec<[f32; 4]> = opaque(&picture())
        .iter()
        .map(|p| p.map(|v| f32::from(v) / 255.0))
        .collect();
    // An above-white sample in the right margin is content.
    px[(3 * W + 11) as usize] = [1.5, 1.0, 1.0, 1.0];
    let img =
        PrecisionImage::from_rgba32_extended(W, H, px.clone(), IccProfile::srgb(), None).unwrap();
    let bits: Vec<[u32; 4]> = px.iter().map(|p| p.map(f32::to_bits)).collect();
    for basis in 0..3u32 {
        for edges in [0, 2, 5, 12, 15] {
            assert_eq!(
                typed_rect(&img, basis as i32, edges),
                expected(&bits, |p| f32::from_bits(p[3]) == 0.0, basis, edges),
                "basis {basis} edges {edges}"
            );
        }
    }
    assert_eq!(typed_rect(&img, 2, 15), Some([2, 1, 10, 6]));
}
