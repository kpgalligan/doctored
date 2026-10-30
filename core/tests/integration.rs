//! Integration tests exercising the public C FFI contract from
//! `include/doctored_core.h`.

use std::ffi::{c_char, c_int, CString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;

use doctored_core::ffi::*;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_painting_layer};
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common;
use common::*;

const FMT_PNG: c_int = 0;
const FMT_JPEG: c_int = 1;
const FMT_TIFF: c_int = 2;
const FMT_BMP: c_int = 3;
const FMT_GIF: c_int = 4;
const FMT_WEBP: c_int = 5;

// ---------------------------------------------------------------- helpers --

/// NOT the shared `common::opaque_pattern` (a different pattern): this one is
/// `test_pattern` with its alpha ramp flattened to 255, and shadows the
/// common helper on purpose so these tests keep their historical pixels.
fn opaque_pattern(w: u32, h: u32) -> RgbaImage {
    let mut img = test_pattern(w, h);
    for px in img.pixels_mut() {
        px[3] = 255;
    }
    img
}

fn save_ok(img: *const DrImage, path: &Path, fmt: c_int, quality: u8) {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_image_save(img, c.as_ptr(), fmt, quality, &mut err) };
    assert!(ok, "save to {path:?} failed: {}", take_err_string(err));
    assert!(err.is_null(), "err_out set on successful save");
}

// ------------------------------------------------------------------ tests --

#[test]
fn png_round_trip_is_exact() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let img = open_pattern(&dir, "pattern.png", &pattern);
    assert_eq!(dims(img), (64, 48));
    assert_eq!(pixels(img), *pattern.as_raw());

    let out = dir.path().join("roundtrip.png");
    save_ok(img, &out, FMT_PNG, 90);
    let img2 = open_ok(&out);
    assert_eq!(dims(img2), (64, 48));
    assert_eq!(pixels(img2), *pattern.as_raw());
    free(img2);
    free(img);
}

#[test]
fn save_and_reopen_every_format() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let img = open_pattern(&dir, "src.png", &pattern);

    for (fmt, ext) in [
        (FMT_PNG, "png"),
        (FMT_JPEG, "jpg"),
        (FMT_TIFF, "tiff"),
        (FMT_BMP, "bmp"),
        (FMT_GIF, "gif"),
        (FMT_WEBP, "webp"),
    ] {
        let path = dir.path().join(format!("out.{ext}"));
        save_ok(img, &path, fmt, 90);
        let reopened = open_ok(&path);
        assert_eq!(dims(reopened), (64, 48), "wrong dims for format {fmt}");
        free(reopened);
    }
    free(img);

    // JPEG decodes to approximately the alpha-over-white composite.
    let jpg = open_ok(&dir.path().join("out.jpg"));
    for (x, y) in [(0u32, 0u32), (63, 0), (0, 47), (63, 47), (32, 24), (10, 40)] {
        let src = pattern.get_pixel(x, y).0;
        let alpha = f32::from(src[3]) / 255.0;
        let got = pixel_at(jpg, x, y);
        for (&g, &s) in got.iter().zip(src.iter()).take(3) {
            let expect = f32::from(s) * alpha + 255.0 * (1.0 - alpha);
            assert!(
                (f32::from(g) - expect).abs() <= 12.0,
                "jpeg pixel ({x},{y}) channel off: got {g}, expected ~{expect}"
            );
        }
    }
    free(jpg);

    // WEBP (lossless), TIFF, and BMP are exact for an opaque image.
    let opaque = opaque_pattern(64, 48);
    let oimg = open_pattern(&dir, "opaque.png", &opaque);
    for (fmt, ext) in [(FMT_WEBP, "webp"), (FMT_TIFF, "tiff"), (FMT_BMP, "bmp")] {
        let path = dir.path().join(format!("opaque.{ext}"));
        save_ok(oimg, &path, fmt, 90);
        let reopened = open_ok(&path);
        assert_eq!(dims(reopened), (64, 48));
        assert_eq!(
            pixels(reopened),
            *opaque.as_raw(),
            "{ext} round-trip not exact for opaque image"
        );
        free(reopened);
    }
    free(oimg);
}

#[test]
fn rotations_and_flips() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let img = open_pattern(&dir, "pattern.png", &pattern);

    // rotate90 is clockwise: WxH -> HxW, top-left -> top-right.
    let r90 = unsafe { dr_image_rotate90(img) };
    assert_eq!(dims(r90), (48, 64));
    let top_left = pattern.get_pixel(0, 0).0;
    assert_eq!(pixel_at(r90, 47, 0), top_left);
    // A distinctive interior pixel: (x, y) -> (H-1-y, x).
    let p53 = pattern.get_pixel(5, 3).0;
    assert_eq!(pixel_at(r90, 48 - 1 - 3, 5), p53);

    // rotate90 twice == rotate180.
    let r90x2 = unsafe { dr_image_rotate90(r90) };
    let r180 = unsafe { dr_image_rotate180(img) };
    assert_eq!(dims(r90x2), (64, 48));
    assert_eq!(dims(r90x2), dims(r180));
    assert_eq!(pixels(r90x2), pixels(r180));

    // rotate90 three times == rotate270.
    let r90x3 = unsafe { dr_image_rotate90(r90x2) };
    let r270 = unsafe { dr_image_rotate270(img) };
    assert_eq!(dims(r90x3), (48, 64));
    assert_eq!(pixels(r90x3), pixels(r270));

    // flip_horizontal twice == identity; single flip moves TL to TR.
    let f1 = unsafe { dr_image_flip_horizontal(img) };
    let f2 = unsafe { dr_image_flip_horizontal(f1) };
    assert_eq!(dims(f1), (64, 48));
    assert_eq!(pixels(f2), pixels(img));
    assert_eq!(pixel_at(f1, 63, 0), top_left);

    // flip_vertical moves TL to BL.
    let fv = unsafe { dr_image_flip_vertical(img) };
    assert_eq!(dims(fv), (64, 48));
    assert_eq!(pixel_at(fv, 0, 47), top_left);

    for p in [r90, r90x2, r90x3, r180, r270, f1, f2, fv, img] {
        free(p);
    }
}

#[test]
fn crop_content_and_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let img = open_pattern(&dir, "pattern.png", &pattern);

    let cropped = unsafe { dr_image_crop(img, 8, 4, 16, 12) };
    assert!(!cropped.is_null());
    assert_eq!(dims(cropped), (16, 12));
    let cp = pixels(cropped);
    for y in 0..12u32 {
        for x in 0..16u32 {
            let src = pattern.get_pixel(8 + x, 4 + y).0;
            let i = ((y * 16 + x) * 4) as usize;
            assert_eq!(&cp[i..i + 4], &src, "crop mismatch at ({x},{y})");
        }
    }
    free(cropped);

    // Full-image crop is the identity.
    let full = unsafe { dr_image_crop(img, 0, 0, 64, 48) };
    assert!(!full.is_null());
    assert_eq!(pixels(full), *pattern.as_raw());
    free(full);

    // Invalid rects -> NULL.
    for (x, y, w, h) in [
        (60, 0, 8, 8),       // extends past right edge
        (0, 40, 8, 16),      // extends past bottom edge
        (0, 0, 0, 5),        // zero width
        (0, 0, 5, 0),        // zero height
        (64, 0, 1, 1),       // starts outside
        (u32::MAX, 0, 2, 2), // x + w overflows u32
        (0, u32::MAX, 1, 2), // y + h overflows u32
        (u32::MAX, u32::MAX, u32::MAX, u32::MAX),
    ] {
        let p = unsafe { dr_image_crop(img, x, y, w, h) };
        assert!(p.is_null(), "crop({x},{y},{w},{h}) should be NULL");
    }
    free(img);
}

#[test]
fn apply_mask_scales_alpha_by_coverage() {
    // Four pixels masked by 0 / 64 / 128 / 255 coverage. Expected alphas are
    // hand-computed round(alpha * coverage / 255) values, independent of the
    // implementation's integer formula.
    let raw: Vec<u8> = vec![
        200, 10, 10, 255, // coverage 0: fully hidden
        10, 200, 10, 100, // coverage 64: round(100 * 64 / 255) = 25
        10, 10, 200, 255, // coverage 128: round(255 * 128 / 255) = 128
        250, 250, 250, 255, // coverage 255: untouched
    ];
    let img = unsafe { dr_image_from_rgba(raw.as_ptr(), 4, 1) };
    assert!(!img.is_null());
    let mask: [u8; 4] = [0, 64, 128, 255];
    let out = unsafe { dr_image_apply_mask(img, mask.as_ptr(), 4, 1) };
    assert!(!out.is_null());
    assert_eq!(dims(out), (4, 1));
    assert_eq!(
        pixels(out),
        vec![
            0, 0, 0, 0, // color drops with the last of the alpha
            10, 200, 10, 25, // straight alpha: color bytes survive the fade
            10, 10, 200, 128, //
            250, 250, 250, 255, // full coverage passes through exactly
        ]
    );
    free(out);

    // Dimensions must match the image exactly.
    assert!(unsafe { dr_image_apply_mask(img, mask.as_ptr(), 2, 2) }.is_null());
    assert!(unsafe { dr_image_apply_mask(img, mask.as_ptr(), 4, 2) }.is_null());
    // NULL mask -> NULL.
    assert!(unsafe { dr_image_apply_mask(img, ptr::null(), 4, 1) }.is_null());
    free(img);
}

#[test]
fn from_rgba_wraps_a_buffer() {
    // The in-memory constructor is how pixels this crate cannot decode
    // itself (a host-decoded HEIC still, a host-rasterized vector page) become an
    // image, so it must produce exactly what the file path produces.
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(9, 5);
    let raw = pattern.as_raw();
    let img = unsafe { dr_image_from_rgba(raw.as_ptr(), 9, 5) };
    assert!(!img.is_null());
    assert_eq!(dims(img), (9, 5));
    assert_eq!(pixels(img), *raw);

    let opened = open_pattern(&dir, "pattern.png", &pattern);
    assert_eq!(pixels(img), pixels(opened));

    // The buffer is COPIED, so overwriting the caller's bytes afterwards
    // cannot reach the handle.
    let mut owned = raw.clone();
    let copy = unsafe { dr_image_from_rgba(owned.as_ptr(), 9, 5) };
    owned.iter_mut().for_each(|b| *b = 0);
    assert_eq!(pixels(copy), *raw);

    // Rejections. Each is caught before any slice is built from the
    // dimensions, so the short buffer below is never read.
    assert!(unsafe { dr_image_from_rgba(raw.as_ptr(), 0, 5) }.is_null());
    assert!(unsafe { dr_image_from_rgba(raw.as_ptr(), 9, 0) }.is_null());
    // 10000 * 10001 = 100_010_000 > 100_000_000, the cap resize enforces.
    assert!(unsafe { dr_image_from_rgba(raw.as_ptr(), 10_000, 10_001) }.is_null());

    for p in [img, opened, copy] {
        free(p);
    }
}

#[test]
fn resize_filters_and_limits() {
    let dir = tempfile::tempdir().unwrap();
    let img = open_pattern(&dir, "pattern.png", &test_pattern(64, 48));

    for filter in [
        FILTER_NEAREST,
        FILTER_BILINEAR,
        FILTER_CATMULL_ROM,
        FILTER_LANCZOS3,
    ] {
        let down = unsafe { dr_image_resize(img, 32, 24, filter) };
        assert!(!down.is_null(), "resize with filter {filter} failed");
        assert_eq!(dims(down), (32, 24));
        free(down);

        let odd = unsafe { dr_image_resize(img, 17, 9, filter) };
        assert!(!odd.is_null());
        assert_eq!(dims(odd), (17, 9));
        free(odd);
    }

    // Nearest-neighbor upscale of a 2x1 keeps exact colors.
    let mut two = RgbaImage::new(2, 1);
    two.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    two.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
    let timg = open_pattern(&dir, "two.png", &two);
    let up = unsafe { dr_image_resize(timg, 4, 2, FILTER_NEAREST) };
    assert!(!up.is_null());
    assert_eq!(dims(up), (4, 2));
    for y in 0..2 {
        for x in 0..2 {
            assert_eq!(pixel_at(up, x, y), [255, 0, 0, 255], "({x},{y}) not red");
        }
        for x in 2..4 {
            assert_eq!(pixel_at(up, x, y), [0, 0, 255, 255], "({x},{y}) not blue");
        }
    }
    free(up);
    free(timg);

    // Invalid targets -> NULL.
    assert!(unsafe { dr_image_resize(img, 0, 10, FILTER_NEAREST) }.is_null());
    assert!(unsafe { dr_image_resize(img, 10, 0, FILTER_NEAREST) }.is_null());
    // 10000 * 10001 = 100_010_000 > 100_000_000.
    assert!(unsafe { dr_image_resize(img, 10_000, 10_001, FILTER_NEAREST) }.is_null());
    // Unknown filter value -> NULL.
    assert!(unsafe { dr_image_resize(img, 10, 10, 99) }.is_null());
    free(img);
}

#[test]
fn adjust_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let img = open_pattern(&dir, "pattern.png", &pattern);

    // Identity.
    let same = unsafe { dr_image_adjust(img, 0.0, 0.0, 0.0) };
    assert!(!same.is_null());
    assert_eq!(pixels(same), *pattern.as_raw());
    free(same);

    // brightness = 1 -> white RGB, alpha unchanged.
    let bright = unsafe { dr_image_adjust(img, 1.0, 0.0, 0.0) };
    let bp = pixels(bright);
    let src = pattern.as_raw();
    for (i, chunk) in bp.chunks_exact(4).enumerate() {
        assert_eq!(&chunk[..3], &[255, 255, 255], "pixel {i} not white");
        assert_eq!(chunk[3], src[i * 4 + 3], "pixel {i} alpha changed");
    }
    free(bright);

    // contrast = -1 -> flat mid gray (128 +/- 1), alpha unchanged.
    let flat = unsafe { dr_image_adjust(img, 0.0, -1.0, 0.0) };
    let fp = pixels(flat);
    for (i, chunk) in fp.chunks_exact(4).enumerate() {
        for &c in &chunk[..3] {
            assert!(
                (127..=129).contains(&c),
                "pixel {i} channel {c} not mid gray"
            );
        }
        assert_eq!(chunk[3], src[i * 4 + 3]);
    }
    free(flat);

    // saturation = -1 -> all channels equal (grayscale).
    let desat = unsafe { dr_image_adjust(img, 0.0, 0.0, -1.0) };
    let dp = pixels(desat);
    for chunk in dp.chunks_exact(4) {
        assert_eq!(chunk[0], chunk[1]);
        assert_eq!(chunk[1], chunk[2]);
    }
    free(desat);

    // Out-of-range values are clamped: brightness 2.0 behaves like 1.0.
    let a = unsafe { dr_image_adjust(img, 2.0, 0.0, 0.0) };
    let b = unsafe { dr_image_adjust(img, 1.0, 0.0, 0.0) };
    assert_eq!(pixels(a), pixels(b));
    free(a);
    free(b);

    free(img);
}

#[test]
fn filters_grayscale_invert_sepia_blur_sharpen() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let img = open_pattern(&dir, "pattern.png", &pattern);
    let src = pattern.as_raw();

    // Grayscale: dims preserved, r == g == b, alpha untouched.
    let gray = unsafe { dr_image_grayscale(img) };
    assert_eq!(dims(gray), (64, 48));
    for (i, chunk) in pixels(gray).chunks_exact(4).enumerate() {
        assert_eq!(chunk[0], chunk[1]);
        assert_eq!(chunk[1], chunk[2]);
        assert_eq!(chunk[3], src[i * 4 + 3]);
    }
    free(gray);

    // Invert twice == identity; single invert flips channels, keeps alpha.
    // `test_pattern`'s last row is alpha 0 with nonzero RGB, so this also pins
    // latent RGB under zero alpha.
    let inv = unsafe { dr_image_invert(img) };
    assert_eq!(dims(inv), (64, 48));
    for (i, chunk) in pixels(inv).chunks_exact(4).enumerate() {
        assert_eq!(chunk[0], 255 - src[i * 4]);
        assert_eq!(chunk[1], 255 - src[i * 4 + 1]);
        assert_eq!(chunk[2], 255 - src[i * 4 + 2]);
        assert_eq!(chunk[3], src[i * 4 + 3]);
    }
    let inv2 = unsafe { dr_image_invert(inv) };
    assert_eq!(pixels(inv2), *src);
    free(inv);
    free(inv2);

    // Sepia: dims preserved, alpha untouched, matrix spot-check.
    let sep = unsafe { dr_image_sepia(img) };
    assert_eq!(dims(sep), (64, 48));
    let sp = pixels(sep);
    for (x, y) in [(0u32, 0u32), (32, 24), (63, 47)] {
        let s = pattern.get_pixel(x, y).0;
        let (r, g, b) = (f32::from(s[0]), f32::from(s[1]), f32::from(s[2]));
        let expect = [
            (0.393 * r + 0.769 * g + 0.189 * b).min(255.0).round() as u8,
            (0.349 * r + 0.686 * g + 0.168 * b).min(255.0).round() as u8,
            (0.272 * r + 0.534 * g + 0.131 * b).min(255.0).round() as u8,
        ];
        let i = ((y * 64 + x) * 4) as usize;
        assert_eq!(&sp[i..i + 3], &expect, "sepia mismatch at ({x},{y})");
        assert_eq!(sp[i + 3], s[3]);
    }
    free(sep);

    // Blur (`dr_image_filter_op` `blur`): dims preserved, actually changes a
    // nonuniform image.
    let byte_blur = |sigma: &str| {
        let op = CString::new("blur").unwrap();
        let params = CString::new(format!(r#"{{"sigma":{sigma}}}"#)).unwrap();
        unsafe {
            doctored_core::ffi_filters::dr_image_filter_op(
                img,
                op.as_ptr(),
                params.as_ptr(),
                ptr::null(),
                ptr::null_mut(),
            )
        }
    };
    let blurred = byte_blur("1.5");
    assert!(!blurred.is_null());
    assert_eq!(dims(blurred), (64, 48));
    assert_ne!(pixels(blurred), *src, "blur changed nothing");
    free(blurred);

    // Invalid blur sigma -> NULL (JSON has no NaN or infinity: a sigma that
    // is not a number, or not JSON at all, is refused too).
    for sigma in ["0", "-1", "NaN", "1e999", "-1e999", "\"2\"", "null"] {
        assert!(byte_blur(sigma).is_null(), "blur({sigma}) should be NULL");
    }

    // Sharpen: dims preserved; amount clamped to 5.0 (10.0 == 5.0).
    let sharp = unsafe { dr_image_sharpen(img, 1.0) };
    assert!(!sharp.is_null());
    assert_eq!(dims(sharp), (64, 48));
    free(sharp);
    let s5 = unsafe { dr_image_sharpen(img, 5.0) };
    let s10 = unsafe { dr_image_sharpen(img, 10.0) };
    assert_eq!(pixels(s5), pixels(s10), "amount not clamped to 5");
    free(s5);
    free(s10);

    // Invalid sharpen amount -> NULL.
    for amount in [0.0f32, -0.5, f32::NAN, f32::INFINITY] {
        assert!(
            unsafe { dr_image_sharpen(img, amount) }.is_null(),
            "sharpen({amount}) should be NULL"
        );
    }

    free(img);
}

#[test]
fn error_paths() {
    let dir = tempfile::tempdir().unwrap();

    // Nonexistent path.
    let missing = cpath(&dir.path().join("does-not-exist.png"));
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_image_open(missing.as_ptr(), &mut err) };
    assert!(img.is_null());
    assert!(!err.is_null(), "no error message for missing file");
    assert!(!take_err_string(err).is_empty());

    // A text file is not an image.
    let txt = dir.path().join("not-an-image.txt");
    std::fs::write(&txt, "this is definitely not an image").unwrap();
    let ctxt = cpath(&txt);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_image_open(ctxt.as_ptr(), &mut err) };
    assert!(img.is_null());
    assert!(!err.is_null());
    assert!(!take_err_string(err).is_empty());

    // err_out == NULL is tolerated on failure.
    let img = unsafe { dr_image_open(missing.as_ptr(), ptr::null_mut()) };
    assert!(img.is_null());

    // Saving to an unwritable path fails with a message and returns false.
    let valid = open_pattern(&dir, "ok.png", &test_pattern(8, 8));
    let bad = CString::new("/nonexistent-dir-xyz-doctored/o.png").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_image_save(valid, bad.as_ptr(), FMT_PNG, 90, &mut err) };
    assert!(!ok);
    assert!(!err.is_null());
    assert!(!take_err_string(err).is_empty());

    // Unknown format value -> false + error.
    let out = cpath(&dir.path().join("weird.bin"));
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_image_save(valid, out.as_ptr(), 42, 90, &mut err) };
    assert!(!ok);
    assert!(!err.is_null());
    assert!(!take_err_string(err).is_empty());

    // Save failure with err_out == NULL is tolerated.
    let ok = unsafe { dr_image_save(valid, bad.as_ptr(), FMT_PNG, 90, ptr::null_mut()) };
    assert!(!ok);

    free(valid);
}

/// Generates tests/fixtures/gradient.psd with ImageMagick if it is missing.
/// Returns None (and logs) if that is impossible, so the suite can skip
/// gracefully instead of failing.
fn psd_fixture() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let path = dir.join("gradient.psd");
    if path.exists() {
        return Some(path);
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("skipping PSD assertions: cannot create fixtures dir: {e}");
        return None;
    }
    let magick = Path::new("/opt/homebrew/bin/magick");
    if !magick.exists() {
        eprintln!("skipping PSD assertions: {} not found", magick.display());
        return None;
    }
    // -depth 8: magick writes gradients as 16-bit by default, which the psd
    // crate (0.3) cannot decode (it panics with an out-of-bounds index).
    match Command::new(magick)
        .args(["-size", "32x24", "gradient:red-blue", "-depth", "8"])
        .arg(&path)
        .status()
    {
        Ok(status) if status.success() && path.exists() => Some(path),
        other => {
            eprintln!("skipping PSD assertions: magick failed: {other:?}");
            None
        }
    }
}

#[test]
fn psd_open_and_corrupt_psd() {
    if let Some(fixture) = psd_fixture() {
        let img = open_ok(&fixture);
        assert_eq!(dims(img), (32, 24));
        let tl = pixel_at(img, 0, 0);
        assert!(
            tl[0] >= 200 && tl[1] <= 80 && tl[2] <= 80,
            "top-left of red-blue gradient not red: {tl:?}"
        );
        free(img);
    }

    // "8BPS" magic followed by garbage must error, not crash.
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.psd");
    let mut bytes = b"8BPS".to_vec();
    bytes.extend_from_slice(&[0xA5; 64]);
    std::fs::write(&bad, bytes).unwrap();
    let cbad = cpath(&bad);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_image_open(cbad.as_ptr(), &mut err) };
    assert!(img.is_null(), "corrupt PSD should not decode");
    assert!(!err.is_null());
    assert!(!take_err_string(err).is_empty());
}

#[test]
fn null_safety_everywhere() {
    let mut authored = [0.25f32; 3];
    assert!(!unsafe {
        doctored_core::ffi_authored_color::dr_authored_color(
            ptr::null(),
            0,
            true,
            ptr::null(),
            authored.as_mut_ptr(),
        )
    });
    assert!(!unsafe {
        doctored_core::ffi_authored_color::dr_authored_color(
            [0u8; 4].as_ptr(),
            4,
            false,
            [0.5f32; 3].as_ptr(),
            ptr::null_mut(),
        )
    });
    assert_eq!(authored, [0.25; 3], "a refusal leaves out untouched");
    // Auto-Blend Layers: a NULL document, request, control and outs.
    assert!(unsafe {
        doctored_core::ffi_auto_blend::dr_doc_auto_blend_plan(
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_auto_blend::dr_doc_auto_blend(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    }
    .is_null());
    // Auto-Align's Cylindrical and Spherical.
    assert!(unsafe {
        doctored_core::ffi_align_projection::dr_doc_project_layer_plan(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_align_projection::dr_doc_project_layer(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_align_projection::dr_precision_doc_project_layer_plan(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_align_projection::dr_precision_doc_project_layer(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    // Auto-Align's Vignette Removal.
    assert!(unsafe {
        doctored_core::ffi_align_vignette::dr_doc_remove_vignette_plan(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_align_vignette::dr_doc_remove_vignette(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_align_vignette::dr_precision_doc_remove_vignette_plan(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_align_vignette::dr_precision_doc_remove_vignette(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    // Auto-Align's Geometric Distortion.
    assert!(unsafe {
        doctored_core::ffi_align_distortion::dr_doc_fit_distortion_plan(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_align_distortion::dr_doc_fit_distortion(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_align_distortion::dr_precision_doc_fit_distortion_plan(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_align_distortion::dr_precision_doc_fit_distortion(
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(!unsafe {
        doctored_core::ffi_feature_register::dr_feature_register(
            ptr::null(),
            ptr::null(),
            64,
            64,
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    });
    assert_eq!(
        unsafe { doctored_core::ffi_feature_register::dr_feature_register_scratch_bytes(0, 0) },
        0
    );
    assert!(unsafe {
        doctored_core::ffi_precision_cutout::dr_precision_doc_refinement_cutout_plan(
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_precision_cutout::dr_precision_doc_add_refinement_cutout(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    // #547 phase 4b: Image ▸ Trim on a CMYK document (NULL document, NULL out).
    let mut trim = [7u32; 4];
    assert!(!unsafe {
        doctored_core::ffi_trim::dr_precision_doc_cmyk_trim_rect(
            ptr::null(),
            0,
            15,
            0,
            0,
            trim.as_mut_ptr(),
            ptr::null_mut(),
        )
    });
    assert_eq!(trim, [7; 4], "a refusal leaves rect_out untouched");
    // #547: Select and Mask's CMYK New Layer.
    assert!(unsafe {
        doctored_core::ffi_cmyk::dr_precision_doc_cmyk_cutout_plan(
            ptr::null(),
            0.0,
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_cmyk::dr_precision_doc_add_cmyk_cutout(
            ptr::null(),
            ptr::null(),
            0.0,
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert_eq!(
        unsafe { doctored_core::ffi_cutout::dr_doc_refinement_cutout_scratch_bytes(ptr::null()) },
        0
    );
    assert!(unsafe {
        doctored_core::ffi_cutout::dr_doc_add_refinement_cutout(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_foreground::dr_image_estimate_foreground(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(!unsafe {
        doctored_core::ffi_mask::dr_doc_layer_masks_equal(ptr::null(), ptr::null(), 0)
    });
    assert!(unsafe {
        doctored_core::ffi_mask::dr_doc_set_layer_content_preserving_mask(
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            0,
            0,
        )
    }
    .is_null());
    unsafe {
        use doctored_core::ffi_mask::*;
        assert!(dr_doc_layer_mask_properties(ptr::null(), 0).is_null());
        assert!(dr_doc_set_mask_properties(ptr::null(), 0, true, 1.0, 0.0).is_null());
        assert!(dr_doc_transform_mask(ptr::null(), 0, ptr::null(), 0).is_null());
        assert!(dr_doc_set_mask(ptr::null(), 0, ptr::null(), 0, 0, 0, 0, false, 0).is_null());
        assert!(dr_doc_set_mask(ptr::null(), 0, [255u8].as_ptr(), 1, 1, 0, 0, true, 0).is_null());
        assert!(dr_doc_invert_mask(ptr::null(), 0, ptr::null(), 0).is_null());
        assert!(dr_doc_invert_mask(ptr::null(), 0, [255u8].as_ptr(), 1).is_null());
        let stops = [
            0.0f32, 0.0, 0.0, 0.0, 1.0, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.5,
        ];
        assert!(doctored_core::ffi_gradient::dr_doc_gradient_stops(
            ptr::null(),
            0,
            0.0,
            0.0,
            1.0,
            1.0,
            stops.as_ptr(),
            2,
            0,
            0,
            1.0,
            ptr::null()
        )
        .is_null());
        assert!(doctored_core::ffi_gradient::dr_doc_gradient_stops(
            ptr::null(),
            0,
            0.0,
            0.0,
            1.0,
            1.0,
            ptr::null(),
            2,
            0,
            0,
            1.0,
            ptr::null()
        )
        .is_null());
    }

    unsafe {
        use doctored_core::ffi_jobs::*;
        dr_job_control_free(ptr::null_mut());
        dr_job_control_cancel(ptr::null());
        assert!(!dr_job_control_cancelled(ptr::null()));
        assert_eq!(dr_job_control_progress(ptr::null()), 0.0);
        assert_eq!(dr_job_control_stage(ptr::null()), 0);
        assert_eq!(dr_job_control_scratch_bytes(ptr::null()), 0);
        assert!(!dr_job_control_set_scratch_limit(ptr::null(), 0));
        assert!(!dr_job_control_set_working_limit(ptr::null(), 0));
        assert!(dr_doc_spot_heal_layer_job(
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            1.0,
            0,
            0,
            0,
            0,
            false,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        let created = dr_job_control_create();
        assert!(!created.is_null());
        dr_job_control_free(created);
    }

    assert!(unsafe {
        doctored_core::ffi_metadata_import::dr_doc_import_metadata(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_metadata_report::dr_path_metadata_import_report(
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_metadata_report::dr_platform_conversion_sentence(
            ptr::null(),
            0,
            99,
            ptr::null_mut(),
        )
    }
    .is_null());
    // NULL bytes are an image with no profile, so a known model still
    // answers its plain sentence.
    let sentence = unsafe {
        doctored_core::ffi_metadata_report::dr_platform_conversion_sentence(
            ptr::null(),
            0,
            0,
            ptr::null_mut(),
        )
    };
    assert!(!sentence.is_null());
    unsafe { dr_string_free(sentence) };
    // NULL clears the working CMYK profile, whatever the length; this
    // binary never sets one, so clearing changes nothing here.
    assert!(unsafe {
        doctored_core::ffi_metadata_report::dr_set_working_cmyk_profile(
            ptr::null(),
            64,
            ptr::null_mut(),
        )
    });
    assert!(unsafe {
        doctored_core::ffi_calibrated::dr_doc_adjust_image(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    let null: *const DrImage = ptr::null();

    assert_eq!(unsafe { dr_image_width(null) }, 0);
    assert_eq!(unsafe { dr_image_height(null) }, 0);
    assert!(unsafe { dr_image_pixels_rgba(null) }.is_null());

    assert!(unsafe { dr_image_rotate90(null) }.is_null());
    assert!(unsafe { dr_image_rotate180(null) }.is_null());
    assert!(unsafe { dr_image_rotate270(null) }.is_null());
    assert!(unsafe { dr_image_flip_horizontal(null) }.is_null());
    assert!(unsafe { dr_image_flip_vertical(null) }.is_null());
    assert!(unsafe { dr_image_crop(null, 0, 0, 1, 1) }.is_null());
    assert!(unsafe { dr_image_apply_mask(null, ptr::null(), 1, 1) }.is_null());
    assert!(unsafe { dr_image_resize(null, 1, 1, FILTER_NEAREST) }.is_null());
    assert!(unsafe { dr_image_adjust(null, 0.0, 0.0, 0.0) }.is_null());
    assert!(unsafe { dr_image_grayscale(null) }.is_null());
    assert!(unsafe { dr_image_invert(null) }.is_null());
    assert!(unsafe {
        doctored_core::ffi_regional::dr_doc_render_region(
            ptr::null(),
            0,
            0,
            1,
            1,
            1,
            4,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        doctored_core::ffi_regional::dr_precision_doc_render_region(
            ptr::null(),
            0,
            0,
            1,
            1,
            1,
            1,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe { dr_image_sepia(null) }.is_null());
    assert!(unsafe { dr_image_sharpen(null, 1.0) }.is_null());
    assert!(unsafe { dr_image_from_rgba(ptr::null(), 1, 1) }.is_null());

    // Saving a NULL image fails cleanly with a message.
    let path = CString::new("/tmp/should-not-be-created.png").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_image_save(null, path.as_ptr(), FMT_PNG, 90, &mut err) };
    assert!(!ok);
    assert!(!take_err_string(err).is_empty());

    // Opening a NULL path fails cleanly with a message.
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_image_open(ptr::null(), &mut err) };
    assert!(img.is_null());
    assert!(!take_err_string(err).is_empty());

    // Free functions are no-ops on NULL.
    unsafe { dr_image_free(ptr::null_mut()) };
    unsafe { dr_string_free(ptr::null_mut()) };
}

#[test]
fn exif_orientation_is_applied() {
    let dir = TempDir::new().unwrap();
    // Encode a 20x10 JPEG through the FFI, then splice in a minimal EXIF APP1
    // segment declaring orientation 6 (stored sideways; rotate 90 CW to view).
    let img = open_pattern(&dir, "base.png", &opaque_pattern(20, 10));
    let jpg = dir.path().join("plain.jpg");
    save_ok(img, &jpg, FMT_JPEG, 95);
    free(img);
    let plain = std::fs::read(&jpg).unwrap();
    assert_eq!(&plain[..2], [0xFF, 0xD8], "JPEG must start with SOI");
    let mut tagged = Vec::with_capacity(plain.len() + 36);
    tagged.extend_from_slice(&plain[..2]);
    tagged.extend_from_slice(&[
        0xFF, 0xE1, 0x00, 0x22, // APP1 marker, segment length 34
        b'E', b'x', b'i', b'f', 0, 0, // Exif header
        0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, // TIFF, little-endian
        0x01, 0x00, // one IFD entry
        0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, // no next IFD
    ]);
    tagged.extend_from_slice(&plain[2..]);
    let tagged_path = dir.path().join("oriented.jpg");
    std::fs::write(&tagged_path, &tagged).unwrap();

    let plain_img = open_ok(&jpg);
    assert_eq!(dims(plain_img), (20, 10), "untagged JPEG keeps stored dims");
    free(plain_img);
    let oriented = open_ok(&tagged_path);
    assert_eq!(
        dims(oriented),
        (10, 20),
        "orientation 6 must transpose dims"
    );
    free(oriented);
}

#[test]
fn sixteen_bit_psd_is_rejected_with_error() {
    let magick = Path::new("/opt/homebrew/bin/magick");
    if !magick.exists() {
        eprintln!("skipping 16-bit PSD assertions: magick not found");
        return;
    }
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("deep.psd");
    let status = Command::new(magick)
        .args(["-size", "16x12", "gradient:red-blue", "-depth", "16"])
        .arg(&path)
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("skipping 16-bit PSD assertions: magick failed");
        return;
    }
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_image_open(c.as_ptr(), &mut err) };
    assert!(
        img.is_null(),
        "16-bit PSD must be rejected, not mis-decoded"
    );
    let msg = take_err_string(err);
    assert!(
        msg.contains("16-bit") && msg.contains("only 8-bit RGB and grayscale"),
        "error should identify the rejected depth and supported import formats: {msg}"
    );
}

#[test]
fn failed_save_preserves_existing_destination() {
    let dir = TempDir::new().unwrap();
    // 20000 px exceeds lossless WebP's maximum side length, so encoding fails
    // after the destination would already have been truncated by a naive save.
    let img = open_pattern(&dir, "wide.png", &opaque_pattern(20000, 1));
    let dest = dir.path().join("out.webp");
    std::fs::write(&dest, b"precious bytes").unwrap();
    let c = cpath(&dest);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_image_save(img, c.as_ptr(), FMT_WEBP, 90, &mut err) };
    assert!(!ok, "expected WebP encode of a 20000-px-wide image to fail");
    let msg = take_err_string(err);
    assert!(!msg.is_empty(), "failure must carry an error message");
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        b"precious bytes",
        "failed save must leave the existing destination intact"
    );
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("dr-tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );
    free(img);
}

// ------------------------------------------------ painting a layer overlay --

const COMPOSITE_OVER: c_int = 0;
const COMPOSITE_ERASE: c_int = 1;

/// A `w`x`h` overlay buffer of PREMULTIPLIED RGBA8, every pixel `px`.
fn solid_overlay(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
    px.iter()
        .copied()
        .cycle()
        .take((w * h * 4) as usize)
        .collect()
}

/// Paints `overlay` onto layer 0 of a document built from `base` through
/// `dr_doc_painting_layer`, returning the painted layer's pixels. The layer
/// never grows, so its dimensions are asserted here for every call.
fn painted_ok(
    dir: &TempDir,
    base: &RgbaImage,
    overlay: &[u8],
    w: u32,
    h: u32,
    mode: c_int,
    alpha: f32,
) -> Vec<u8> {
    assert_eq!(
        overlay.len(),
        (w * h * 4) as usize,
        "test overlay sized wrong"
    );
    let doc = doc_from(dir, "base.png", base);
    let out = unsafe { dr_doc_painting_layer(doc, 0, overlay.as_ptr(), w, h, mode, alpha) };
    assert!(
        !out.is_null(),
        "painting_layer(mode {mode}, alpha {alpha}) failed"
    );
    assert_eq!(layer_dims(out, 0), (w, h));
    let painted = layer_pixels(out, 0);
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
    painted
}

/// Layer 0's pixels of a document built from `base`, unpainted — what a
/// painted layer is compared against.
fn unpainted(dir: &TempDir, base: &RgbaImage) -> Vec<u8> {
    let doc = doc_from(dir, "base.png", base);
    let pixels = layer_pixels(doc, 0);
    unsafe { dr_doc_free(doc) };
    pixels
}

#[test]
fn painting_layer_transparent_overlay_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let before = unpainted(&dir, &pattern);
    let overlay = vec![0u8; 64 * 48 * 4];
    for mode in [COMPOSITE_OVER, COMPOSITE_ERASE] {
        let out = painted_ok(&dir, &pattern, &overlay, 64, 48, mode, 1.0);
        assert_eq!(
            out, before,
            "transparent overlay (mode {mode}) must be byte-identical"
        );
    }
}

#[test]
fn painting_layer_opaque_overlay_replaces_everything() {
    let dir = tempfile::tempdir().unwrap();
    let red = solid_overlay(64, 48, [255, 0, 0, 255]);
    let out = painted_ok(
        &dir,
        &test_pattern(64, 48),
        &red,
        64,
        48,
        COMPOSITE_OVER,
        1.0,
    );
    for (i, chunk) in out.chunks_exact(4).enumerate() {
        assert_eq!(chunk, &[255, 0, 0, 255], "pixel {i} not opaque red");
    }
}

#[test]
fn painting_layer_half_red_over_white() {
    let dir = tempfile::tempdir().unwrap();
    let white = RgbaImage::from_pixel(16, 12, Rgba([255, 255, 255, 255]));
    // Premultiplied half-transparent red.
    let overlay = solid_overlay(16, 12, [128, 0, 0, 128]);
    let out = painted_ok(&dir, &white, &overlay, 16, 12, COMPOSITE_OVER, 1.0);
    for (i, chunk) in out.chunks_exact(4).enumerate() {
        let expect = [255i32, 127, 127, 255];
        for (c, (&got, want)) in chunk.iter().zip(expect).enumerate() {
            assert!(
                (i32::from(got) - want).abs() <= 2,
                "pixel {i} channel {c}: got {got}, want ~{want}"
            );
        }
    }
}

#[test]
fn painting_layer_sub_rect_leaves_outside_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = opaque_pattern(64, 48);
    // Half-transparent red only inside x 8..24, y 4..16; transparent outside.
    let (rx, ry, rw, rh) = (8u32, 4u32, 16u32, 12u32);
    let mut overlay = vec![0u8; 64 * 48 * 4];
    for y in ry..ry + rh {
        for x in rx..rx + rw {
            let i = ((y * 64 + x) * 4) as usize;
            overlay[i..i + 4].copy_from_slice(&[128, 0, 0, 128]);
        }
    }
    let op = painted_ok(&dir, &pattern, &overlay, 64, 48, COMPOSITE_OVER, 1.0);
    let src = unpainted(&dir, &pattern);
    let sa = 128.0f32 / 255.0;
    for y in 0..48u32 {
        for x in 0..64u32 {
            let i = ((y * 64 + x) * 4) as usize;
            let inside = (rx..rx + rw).contains(&x) && (ry..ry + rh).contains(&y);
            if !inside {
                assert_eq!(
                    &op[i..i + 4],
                    &src[i..i + 4],
                    "outside pixel ({x},{y}) changed"
                );
                continue;
            }
            // Destination is opaque, so out_a = 1 and
            // out_c = scp + dc * (1 - sa).
            let scp = [128.0f32 / 255.0, 0.0, 0.0];
            for c in 0..3 {
                let dc = f32::from(src[i + c]) / 255.0;
                let want = ((scp[c] + dc * (1.0 - sa)) * 255.0).round();
                let got = f32::from(op[i + c]);
                assert!(
                    (got - want).abs() <= 2.0,
                    "inside pixel ({x},{y}) channel {c}: got {got}, want ~{want}"
                );
            }
            assert_eq!(op[i + 3], 255, "inside pixel ({x},{y}) alpha");
        }
    }
}

#[test]
fn painting_layer_alpha_parameter_scales_and_clamps() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let opaque_red = solid_overlay(64, 48, [255, 0, 0, 255]);
    let half_red = solid_overlay(64, 48, [128, 0, 0, 128]);
    let paint = |overlay: &[u8], alpha: f32| {
        painted_ok(&dir, &pattern, overlay, 64, 48, COMPOSITE_OVER, alpha)
    };

    // alpha 0.5 with an opaque overlay ~= overlay alpha 128 at alpha 1.0.
    let scaled = paint(&opaque_red, 0.5);
    let baked = paint(&half_red, 1.0);
    for (i, (&a, &b)) in scaled.iter().zip(baked.iter()).enumerate() {
        assert!(
            (i32::from(a) - i32::from(b)).abs() <= 2,
            "byte {i}: alpha 0.5 gave {a}, overlay 128 gave {b}"
        );
    }

    // Out-of-range alpha clamps to the [0, 1] endpoints exactly.
    let hi = paint(&opaque_red, 1.5);
    let one = paint(&opaque_red, 1.0);
    assert_eq!(hi, one, "alpha 1.5 must behave like 1.0");
    let lo = paint(&opaque_red, -0.5);
    let zero = paint(&opaque_red, 0.0);
    assert_eq!(lo, zero, "alpha -0.5 must behave like 0.0");

    // NaN alpha -> NULL.
    let doc = doc_from(&dir, "base.png", &pattern);
    let p = unsafe {
        dr_doc_painting_layer(
            doc,
            0,
            opaque_red.as_ptr(),
            64,
            48,
            COMPOSITE_OVER,
            f32::NAN,
        )
    };
    assert!(p.is_null(), "NaN alpha must return NULL");
    unsafe { dr_doc_free(doc) };
}

#[test]
fn painting_layer_erase_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let pattern = test_pattern(64, 48);
    let src = unpainted(&dir, &pattern);

    // Opaque overlay erases all alpha; color bytes unchanged.
    let full = solid_overlay(64, 48, [77, 200, 13, 255]);
    let erased = painted_ok(&dir, &pattern, &full, 64, 48, COMPOSITE_ERASE, 1.0);
    for (i, chunk) in erased.chunks_exact(4).enumerate() {
        assert_eq!(
            &chunk[..3],
            &src[i * 4..i * 4 + 3],
            "pixel {i} color changed"
        );
        assert_eq!(chunk[3], 0, "pixel {i} alpha not erased");
    }

    // Alpha-128 overlay (any color) halves destination alpha; color unchanged.
    let half = solid_overlay(64, 48, [64, 32, 128, 128]);
    let halved = painted_ok(&dir, &pattern, &half, 64, 48, COMPOSITE_ERASE, 1.0);
    for (i, chunk) in halved.chunks_exact(4).enumerate() {
        assert_eq!(
            &chunk[..3],
            &src[i * 4..i * 4 + 3],
            "pixel {i} color changed"
        );
        let want = f32::from(src[i * 4 + 3]) / 2.0;
        assert!(
            (f32::from(chunk[3]) - want).abs() <= 1.0,
            "pixel {i} alpha: got {}, want ~{want}",
            chunk[3]
        );
    }
}

#[test]
fn painting_layer_guards() {
    let dir = tempfile::tempdir().unwrap();
    let doc = doc_from(&dir, "base.png", &test_pattern(64, 48));
    // Oversized buffer so mismatched-dimension calls could never read past it
    // even if the guard were broken.
    let overlay = solid_overlay(65, 49, [0, 0, 0, 255]);

    // Dimension mismatches -> NULL.
    for (w, h) in [(65u32, 48u32), (64, 49), (63, 48), (64, 47), (0, 0)] {
        let p =
            unsafe { dr_doc_painting_layer(doc, 0, overlay.as_ptr(), w, h, COMPOSITE_OVER, 1.0) };
        assert!(p.is_null(), "dims ({w},{h}) must return NULL");
    }

    // NULL src -> NULL.
    let p = unsafe { dr_doc_painting_layer(doc, 0, ptr::null(), 64, 48, COMPOSITE_OVER, 1.0) };
    assert!(p.is_null(), "NULL src must return NULL");

    // NULL doc -> NULL.
    let p = unsafe {
        dr_doc_painting_layer(
            ptr::null(),
            0,
            overlay.as_ptr(),
            64,
            48,
            COMPOSITE_OVER,
            1.0,
        )
    };
    assert!(p.is_null(), "NULL doc must return NULL");

    // Unknown mode -> NULL.
    let good = solid_overlay(64, 48, [0, 0, 0, 255]);
    let p = unsafe { dr_doc_painting_layer(doc, 0, good.as_ptr(), 64, 48, 99, 1.0) };
    assert!(p.is_null(), "mode 99 must return NULL");

    unsafe { dr_doc_free(doc) };
}

#[test]
fn painting_layer_over_transparent_destination_unpremultiplies() {
    let dir = tempfile::tempdir().unwrap();
    // Fully transparent destination with junk color bytes underneath.
    let clear = RgbaImage::from_pixel(16, 12, Rgba([9, 200, 33, 0]));
    let overlay = solid_overlay(16, 12, [30, 200, 90, 255]);
    let out = painted_ok(&dir, &clear, &overlay, 16, 12, COMPOSITE_OVER, 1.0);
    for (i, chunk) in out.chunks_exact(4).enumerate() {
        assert_eq!(
            chunk,
            &[30, 200, 90, 255],
            "pixel {i}: opaque overlay over transparent dst must show its straight color"
        );
    }
}
