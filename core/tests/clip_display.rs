//! The clipping display, black-box through the FFI. The oracle is an
//! independent per-pixel rule written here: highlights start from an opaque
//! black ground and light each RGB channel at the maximum code; shadows start
//! from an opaque white ground and drop each RGB channel at zero; an alpha-0
//! pixel stays ground. The small byte case is also spelled out by hand.
//! At f32 the rule is written over linear values: a channel at or above 1.0
//! is at the maximum, one at or below 0.0 at zero, and a pixel whose alpha
//! is not positive shows the ground.
use doctored_core::ffi::{
    dr_image_free, dr_image_from_rgba, dr_image_height, dr_image_pixels_rgba, dr_image_width,
    dr_string_free,
};
use doctored_core::ffi_clip_display::{dr_image_clip_display, dr_precision_image_clip_display};
use doctored_core::ffi_precision::{
    dr_precision_doc_render, dr_precision_image_free, dr_precision_image_height,
    dr_precision_image_pixels_rgba16, dr_precision_image_width,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::FloatTiff;
use doctored_core::DrImage;
use serde_json::json;
use std::ffi::{c_char, CStr};
use std::ptr;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const HIGHLIGHTS: u32 = 1;
const SHADOWS: u32 = 2;

fn take(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    text
}

/// The independent rule, for any sample type.
fn oracle<T: PartialEq + Copy>(p: [T; 4], zero: T, max: T, mode: u32) -> [u8; 4] {
    let highlights = mode == HIGHLIGHTS;
    let mut out = if highlights {
        [0, 0, 0, 255]
    } else {
        [255, 255, 255, 255]
    };
    if p[3] != zero {
        for c in 0..3 {
            if highlights && p[c] == max {
                out[c] = 255;
            }
            if !highlights && p[c] == zero {
                out[c] = 0;
            }
        }
    }
    out
}

/// Consume a byte result into its pixels.
fn bytes(image: *mut DrImage) -> (u32, u32, Vec<[u8; 4]>) {
    assert!(!image.is_null());
    let (w, h) = unsafe { (dr_image_width(image), dr_image_height(image)) };
    let raw =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), (w * h * 4) as usize) };
    let pixels = raw
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    unsafe { dr_image_free(image) };
    (w, h, pixels)
}

fn image16(width: u32, height: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

fn typed(image: &PrecisionImage, mode: u32) -> Result<(u32, u32, Vec<[u8; 4]>), String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_image_clip_display(image, mode, LIMIT, 0, &mut err) };
    if out.is_null() {
        return Err(take(err));
    }
    assert!(err.is_null());
    Ok(bytes(out))
}

const MIXED: [[u8; 4]; 8] = [
    [255, 0, 128, 255],
    [255, 255, 255, 255],
    [0, 0, 0, 255],
    [255, 0, 255, 0],
    [254, 1, 0, 10],
    [0, 255, 0, 255],
    [128, 128, 128, 255],
    [0, 0, 0, 0],
];

#[test]
fn byte_modes_threshold_each_channel_and_ignore_alpha_zero() {
    let flat: Vec<u8> = MIXED.iter().flatten().copied().collect();
    let source = unsafe { dr_image_from_rgba(flat.as_ptr(), 4, 2) };
    assert!(!source.is_null());
    let highlights = [
        [255, 0, 0, 255],
        [255, 255, 255, 255],
        [0, 0, 0, 255],
        [0, 0, 0, 255],
        [0, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 0, 255],
        [0, 0, 0, 255],
    ];
    let shadows = [
        [255, 0, 255, 255],
        [255, 255, 255, 255],
        [0, 0, 0, 255],
        [255, 255, 255, 255],
        [255, 255, 0, 255],
        [0, 255, 0, 255],
        [255, 255, 255, 255],
        [255, 255, 255, 255],
    ];
    for (mode, expected) in [(HIGHLIGHTS, highlights), (SHADOWS, shadows)] {
        let (w, h, pixels) = bytes(unsafe { dr_image_clip_display(source, mode) });
        assert_eq!((w, h), (4, 2));
        assert_eq!(pixels, expected, "mode {mode}");
        let oracle: Vec<[u8; 4]> = MIXED.iter().map(|&p| oracle(p, 0, 255, mode)).collect();
        assert_eq!(pixels, oracle, "mode {mode} against the rule");
    }
    for bad in [0, 4] {
        assert!(unsafe { dr_image_clip_display(source, bad) }.is_null());
    }
    assert!(unsafe { dr_image_clip_display(ptr::null(), HIGHLIGHTS) }.is_null());
    unsafe { dr_image_free(source) };
}

#[test]
fn u16_flags_only_the_exact_native_endpoints() {
    let samples = vec![
        [65535, 65534, 0, 65535],
        [1, 0, 65535, 65535],
        [65534, 1, 32768, 65535],
        [65535, 0, 65535, 0],
    ];
    let source = image16(4, 1, samples.clone());
    let (w, h, high) = typed(&source, HIGHLIGHTS).unwrap();
    assert_eq!((w, h), (4, 1));
    assert_eq!(
        high,
        [
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
        ]
    );
    let (_, _, low) = typed(&source, SHADOWS).unwrap();
    assert_eq!(
        low,
        [
            [255, 255, 0, 255],
            [255, 0, 255, 255],
            [255, 255, 255, 255],
            [255, 255, 255, 255],
        ]
    );
    for mode in [HIGHLIGHTS, SHADOWS] {
        let (_, _, pixels) = typed(&source, mode).unwrap();
        let oracle: Vec<[u8; 4]> = samples.iter().map(|&p| oracle(p, 0, 65535, mode)).collect();
        assert_eq!(pixels, oracle);
    }
}

#[test]
fn a_tiled_render_matches_the_contiguous_oracle() {
    let (w, h) = (600u32, 520u32);
    let samples: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let code = |k: u32| match (x * 31 + y * 17 + k * 7) % 9 {
                0 => 0,
                1 => 65535,
                2 => 65534,
                3 => 1,
                v => (v * 7000 + x + y) as u16,
            };
            [code(0), code(1), code(2), 65535]
        })
        .collect();
    let layers = vec![Layer::raster("Base", image16(w, h, samples), (0, 0), 1.0, true).unwrap()];
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let render = unsafe { dr_precision_doc_render(&doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!render.is_null(), "render: {}", take(err));
    assert_eq!(
        unsafe {
            (
                dr_precision_image_width(render),
                dr_precision_image_height(render),
            )
        },
        (w, h)
    );
    let lent = unsafe { dr_precision_image_pixels_rgba16(render) };
    assert!(!lent.is_null());
    let contiguous: Vec<[u16; 4]> =
        unsafe { std::slice::from_raw_parts(lent, (w * h * 4) as usize) }
            .chunks_exact(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect();
    for mode in [HIGHLIGHTS, SHADOWS] {
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe { dr_precision_image_clip_display(render, mode, LIMIT, 0, &mut err) };
        assert!(!out.is_null(), "clip display: {}", take(err));
        let (ow, oh, pixels) = bytes(out);
        assert_eq!((ow, oh), (w, h));
        let oracle: Vec<[u8; 4]> = contiguous
            .iter()
            .map(|&p| oracle(p, 0, 65535, mode))
            .collect();
        let flagged = oracle.iter().filter(|p| **p != oracle_ground(mode)).count();
        assert!(flagged > 0, "the pattern reaches the endpoint");
        assert!(pixels == oracle, "mode {mode} differs from the oracle");
    }
    unsafe { dr_precision_image_free(render) };
}

fn oracle_ground(mode: u32) -> [u8; 4] {
    if mode == HIGHLIGHTS {
        [0, 0, 0, 255]
    } else {
        [255, 255, 255, 255]
    }
}

#[test]
fn refusals_name_their_reason() {
    let source = image16(1, 1, vec![[65535, 0, 0, 65535]]);
    let float = image32(1, 1, vec![[1.0, 0.0, 0.5, 1.0]]);
    for bad in [0, 4] {
        assert!(!typed(&source, bad).unwrap_err().is_empty(), "mode {bad}");
        assert!(
            !typed(&float, bad).unwrap_err().is_empty(),
            "f32 mode {bad}"
        );
    }
    for image in [&source, &float] {
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe { dr_precision_image_clip_display(image, HIGHLIGHTS, 1, 0, &mut err) };
        assert!(out.is_null());
        assert!(!take(err).is_empty(), "admission refuses with a reason");
    }
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_precision_image_clip_display(ptr::null(), HIGHLIGHTS, LIMIT, 0, &mut err) };
    assert!(out.is_null());
    assert!(!take(err).is_empty());
}

const OVERLAY: u32 = 3;

/// The independent overlay rule: clear unless a visible pixel
/// has a channel at an endpoint; blue for zero, red for the maximum,
/// magenta for both.
fn overlay_oracle<T: PartialEq + Copy>(p: [T; 4], zero: T, max: T) -> [u8; 4] {
    if p[3] == zero {
        return [0, 0, 0, 0];
    }
    let low = (0..3).any(|c| p[c] == zero);
    let high = (0..3).any(|c| p[c] == max);
    match (low, high) {
        (true, true) => [255, 0, 255, 255],
        (true, false) => [0, 0, 255, 255],
        (false, true) => [255, 0, 0, 255],
        (false, false) => [0, 0, 0, 0],
    }
}

#[test]
fn byte_overlay_marks_endpoints_and_leaves_the_rest_clear() {
    let flat: Vec<u8> = MIXED.iter().flatten().copied().collect();
    let source = unsafe { dr_image_from_rgba(flat.as_ptr(), 4, 2) };
    assert!(!source.is_null());
    let (w, h, pixels) = bytes(unsafe { dr_image_clip_display(source, OVERLAY) });
    assert_eq!((w, h), (4, 2));
    assert_eq!(
        pixels,
        [
            [255, 0, 255, 255],
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [0, 0, 0, 0],
            [0, 0, 255, 255],
            [255, 0, 255, 255],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
        ]
    );
    let oracle: Vec<[u8; 4]> = MIXED.iter().map(|&p| overlay_oracle(p, 0, 255)).collect();
    assert_eq!(pixels, oracle);
    unsafe { dr_image_free(source) };
}

#[test]
fn u16_overlay_tests_exact_native_endpoints() {
    let samples = vec![
        [65534, 1, 32768, 65535],
        [65535, 1, 2, 65535],
        [1, 0, 65534, 65535],
        [0, 65535, 9, 65535],
        [65535, 0, 65535, 0],
        [1, 1, 65534, 1],
    ];
    let source = image16(6, 1, samples.clone());
    let (w, h, pixels) = typed(&source, OVERLAY).unwrap();
    assert_eq!((w, h), (6, 1));
    assert_eq!(
        pixels,
        [
            [0, 0, 0, 0],
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [255, 0, 255, 255],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
        ]
    );
    let oracle: Vec<[u8; 4]> = samples
        .iter()
        .map(|&p| overlay_oracle(p, 0, 65535))
        .collect();
    assert_eq!(pixels, oracle);
}

fn image32(width: u32, height: u32, pixels: Vec<[f32; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba32_extended(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

/// The independent float rule, written over linear values: a visible pixel
/// (alpha > 0) has a channel at the maximum when it is >= 1.0 and at zero
/// when it is <= 0.0; Overlay marks as the code rule does.
fn float_oracle(p: [f32; 4], mode: u32) -> [u8; 4] {
    let visible = p[3] > 0.0;
    let high = |c: usize| visible && p[c] >= 1.0;
    let low = |c: usize| visible && p[c] <= 0.0;
    match mode {
        HIGHLIGHTS => [0, 1, 2].map(|c| if high(c) { 255 } else { 0 }),
        SHADOWS => [0, 1, 2].map(|c| if low(c) { 0 } else { 255 }),
        _ => {
            let (l, h) = ((0..3).any(low), (0..3).any(high));
            return match (l, h) {
                (true, true) => [255, 0, 255, 255],
                (true, false) => [0, 0, 255, 255],
                (false, true) => [255, 0, 0, 255],
                (false, false) => [0, 0, 0, 0],
            };
        }
    }
    .into_iter()
    .chain([255])
    .collect::<Vec<u8>>()
    .try_into()
    .unwrap()
}

#[test]
fn f32_tests_linear_values_at_and_past_the_endpoints() {
    let samples = vec![
        [1.0, 0.9999, 1e-6, 1.0],
        [0.0, -0.5, 2.5, 1.0],
        [1.5, 0.0, 0.5, 0.5],
        [2.0, -1.0, 0.0, 0.0],
        [0.9999, 1e-6, 0.5, 1.0],
        [-0.0, 1.0, 1.0, 1e-6],
    ];
    let source = image32(6, 1, samples.clone());
    let (w, h, high) = typed(&source, HIGHLIGHTS).unwrap();
    assert_eq!((w, h), (6, 1));
    assert_eq!(
        high,
        [
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [255, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 255, 255, 255],
        ]
    );
    let (_, _, low) = typed(&source, SHADOWS).unwrap();
    assert_eq!(
        low,
        [
            [255, 255, 255, 255],
            [0, 0, 255, 255],
            [255, 0, 255, 255],
            [255, 255, 255, 255],
            [255, 255, 255, 255],
            [0, 255, 255, 255],
        ]
    );
    let (_, _, overlay) = typed(&source, OVERLAY).unwrap();
    assert_eq!(
        overlay,
        [
            [255, 0, 0, 255],
            [255, 0, 255, 255],
            [255, 0, 255, 255],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [255, 0, 255, 255],
        ]
    );
    for mode in [HIGHLIGHTS, SHADOWS, OVERLAY] {
        let (_, _, pixels) = typed(&source, mode).unwrap();
        let oracle: Vec<[u8; 4]> = samples.iter().map(|&p| float_oracle(p, mode)).collect();
        assert_eq!(pixels, oracle, "mode {mode}");
    }
    // A NaN alpha never reaches the display: the float image refuses it.
    assert!(PrecisionImage::from_rgba32_extended(
        1,
        1,
        vec![[1.0, 0.0, 0.0, f32::NAN]],
        IccProfile::srgb(),
        None
    )
    .is_err());
}

#[test]
fn a_tiled_f32_image_matches_the_contiguous_oracle() {
    let (w, h) = (600u32, 520u32);
    let samples: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let value = |k: u32| match (x * 31 + y * 17 + k * 7) % 9 {
                0 => 0.0,
                1 => 1.0,
                2 => 0.9999,
                3 => 1e-6,
                4 => -0.3,
                5 => 1.7,
                v => (v * 1000 + x + y) as f32 / 10000.0,
            };
            let alpha = match (x + y) % 13 {
                0 => 0.0,
                1 => 0.5,
                _ => 1.0,
            };
            [value(0), value(1), value(2), alpha]
        })
        .collect();
    let source = image32(w, h, samples.clone());
    for mode in [HIGHLIGHTS, SHADOWS, OVERLAY] {
        let (ow, oh, pixels) = typed(&source, mode).unwrap();
        assert_eq!((ow, oh), (w, h));
        let oracle: Vec<[u8; 4]> = samples.iter().map(|&p| float_oracle(p, mode)).collect();
        assert!(pixels == oracle, "mode {mode} differs from the oracle");
    }
}

/// A 32-bit float document of one `width` × 1 raster at alpha 1, through a
/// float TIFF opened with `open_preserving`.
fn float_row_document(rows: &[[f32; 3]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width: rows.len() as u32,
        height: 1,
        channels: 4,
        alpha: Some(false),
        samples: rows.iter().flat_map(|p| [p[0], p[1], p[2], 1.0]).collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("clip.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// Float Curves holds an inside black end point flat at exactly its output,
/// so every input below the (64, 0) point lands on 0.0 and Shadows must drop
/// it: the reason the float test is inclusive.
#[test]
fn f32_shadows_drop_what_a_float_curve_crushes_at_its_black_point() {
    let point = 64.0_f32 / 255.0;
    let rows: Vec<[f32; 3]> = (0..48)
        .map(|i| {
            let v = i as f32 / 40.0;
            [v, v * 0.5, 1.3 - v]
        })
        .collect();
    let doc = float_row_document(&rows);
    let description = json!({"type":"adjust","evaluation":"precision_v1","op":"curves",
        "params":{"rgb": [[64, 0], [255, 255]]}})
    .to_string();
    let curved = doc
        .add_adjustment("Curves", &description, 1.0, LIMIT, 0, None)
        .expect("the adjustment is added")
        .expect("the stack changed");
    let mut err: *mut c_char = ptr::null_mut();
    let render = unsafe { dr_precision_doc_render(&curved, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!render.is_null(), "render: {}", take(err));
    let rendered = unsafe { &*render }.to_rgba32().unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_image_clip_display(render, SHADOWS, LIMIT, 0, &mut err) };
    assert!(!out.is_null(), "clip display: {}", take(err));
    let (_, _, pixels) = bytes(out);
    let mut crushed = 0;
    for (i, input) in rows.iter().enumerate() {
        for c in 0..3 {
            if input[c] < point {
                crushed += 1;
                assert_eq!(rendered[i][c], 0.0, "input {} crushes to 0.0", input[c]);
                assert_eq!(pixels[i][c], 0, "pixel {i} channel {c} is dropped");
            } else if input[c] > point + 0.01 {
                assert_eq!(pixels[i][c], 255, "pixel {i} channel {c} is kept");
            }
        }
    }
    assert!(crushed > 0, "the row reaches below the black point");
    let oracle: Vec<[u8; 4]> = rendered.iter().map(|&p| float_oracle(p, SHADOWS)).collect();
    assert_eq!(pixels, oracle);
    unsafe { dr_precision_image_free(render) };
}
