//! Select ▸ Focus Area through `dr_selection_focus_area`: a sharp half is
//! selected and a blurred half is not, an enclosed flat area is filled,
//! transparent pixels stay outside, `range` moves the cut monotonically, Auto
//! replays through its own reported range, a flat picture selects nothing and
//! a picture sharp everywhere is one class. The blur is a box written here;
//! every expectation is a region of the drawn picture, never a golden mask.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_focus_area::dr_selection_focus_area;
use doctored_core::DrImage;

const W: u32 = 96;
const H: u32 = 64;

struct Image(*mut DrImage);

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { dr_image_free(self.0) };
    }
}

fn image(f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
    let bytes: Vec<u8> = (0..W * H).flat_map(|i| f(i % W, i / W)).collect();
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), W, H) };
    assert!(!img.is_null());
    Image(img)
}

/// The fine texture: a one-pixel checker of two greys.
fn checker(x: u32, y: u32) -> u8 {
    if (x + y).is_multiple_of(2) {
        60
    } else {
        190
    }
}

/// `checker` box-blurred over a 7×7 window, clamped at the canvas: the same
/// texture with its fine detail gone.
fn blurred(x: u32, y: u32) -> u8 {
    let (mut sum, mut n) = (0u32, 0u32);
    for dy in -3i64..=3 {
        for dx in -3i64..=3 {
            let sx = (i64::from(x) + dx).clamp(0, i64::from(W) - 1) as u32;
            let sy = (i64::from(y) + dy).clamp(0, i64::from(H) - 1) as u32;
            sum += u32::from(checker(sx, sy));
            n += 1;
        }
    }
    ((sum + n / 2) / n) as u8
}

fn grey(v: u8) -> [u8; 4] {
    [v, v, v, 255]
}

/// `(mask, range)`, or the error.
fn focus(img: &Image, json: &str) -> Result<(Vec<u8>, f64), String> {
    let params = CString::new(json).unwrap();
    let mut mask = vec![0u8; (W * H) as usize];
    let mut range = f64::NAN;
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_selection_focus_area(
            img.0,
            params.as_ptr(),
            mask.as_mut_ptr(),
            &mut range,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        assert!(err.is_null());
        return Ok((mask, range));
    }
    assert!(!err.is_null(), "a refusal without a message");
    let message = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    Err(message)
}

fn at(mask: &[u8], x: u32, y: u32) -> u8 {
    mask[(y * W + x) as usize]
}

/// The vote box, the bands and the feather reach about 20 px from the seam.
const SEAM: u32 = 24;

fn half_sharp() -> Image {
    image(|x, y| {
        grey(if x < W / 2 {
            checker(x, y)
        } else {
            blurred(x, y)
        })
    })
}

#[test]
fn the_sharp_half_is_selected_and_the_blurred_half_is_not() {
    let img = half_sharp();
    for json in [r#"{}"#, r#"{"soften_edge":false}"#] {
        let (mask, range) = focus(&img, json).unwrap();
        assert!((0.0..=100.0).contains(&range), "{range}");
        for y in 0..H {
            for x in 0..W / 2 - SEAM {
                assert!(
                    at(&mask, x, y) >= 250,
                    "{json}: sharp ({x}, {y}) is {}",
                    at(&mask, x, y)
                );
            }
            for x in W / 2 + SEAM..W {
                assert_eq!(at(&mask, x, y), 0, "{json}: blurred ({x}, {y})");
            }
        }
    }
}

#[test]
fn an_enclosed_flat_area_is_filled() {
    // A sharp ring around a flat disc, on a flat ground.
    let img = image(|x, y| {
        let d = ((f64::from(x) - 48.0).powi(2) + (f64::from(y) - 32.0).powi(2)).sqrt();
        grey(if (14.0..26.0).contains(&d) {
            checker(x, y)
        } else {
            128
        })
    });
    let (mask, _) = focus(&img, r#"{"soften_edge":false}"#).unwrap();
    for (x, y) in [(48, 32), (44, 30), (52, 35)] {
        assert_eq!(at(&mask, x, y), 255, "the disc's ({x}, {y})");
    }
    for (x, y) in [(0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1), (5, 32)] {
        assert_eq!(at(&mask, x, y), 0, "the ground's ({x}, {y})");
    }
}

#[test]
fn transparent_pixels_stay_outside() {
    // A transparent square inside the sharp half; its hidden colour is the
    // same texture.
    let hole = |x: u32, y: u32| (6..18).contains(&x) && (20..40).contains(&y);
    let img = image(|x, y| {
        let mut p = grey(if x < W / 2 {
            checker(x, y)
        } else {
            blurred(x, y)
        });
        if hole(x, y) {
            p[3] = 0;
        }
        p
    });
    for json in [r#"{}"#, r#"{"soften_edge":false}"#] {
        let (mask, _) = focus(&img, json).unwrap();
        for y in 0..H {
            for x in 0..W {
                if hole(x, y) {
                    assert_eq!(at(&mask, x, y), 0, "{json}: transparent ({x}, {y})");
                }
            }
        }
        assert!(at(&mask, 2, 10) >= 250, "{json}");
    }
}

#[test]
fn range_moves_the_cut_and_auto_replays_through_its_range() {
    let img = image(|x, y| {
        // Detail fading from left to right: the cut moves with range.
        let v = checker(x, y);
        let keep = 1.0 - f64::from(x) / f64::from(W);
        grey((128.0 + (f64::from(v) - 128.0) * keep).round() as u8)
    });
    let mut previous = u64::MAX;
    for range in [0, 20, 40, 60, 80, 100] {
        let (mask, used) =
            focus(&img, &format!(r#"{{"range":{range},"soften_edge":false}}"#)).unwrap();
        assert_eq!(used, f64::from(range));
        let selected = mask.iter().filter(|&&v| v > 0).count() as u64;
        assert!(
            selected <= previous,
            "range {range}: {selected} > {previous}"
        );
        previous = selected;
    }
    let half = half_sharp();
    let (auto, range) = focus(&half, r#"{}"#).unwrap();
    let (replayed, used) = focus(&half, &format!(r#"{{"range":{range}}}"#)).unwrap();
    assert_eq!(used, range);
    assert_eq!(replayed, auto, "range {range} did not replay Auto");
}

#[test]
fn a_flat_picture_selects_nothing_and_a_sharp_one_everything() {
    let flat = image(|_, _| grey(128));
    let (mask, range) = focus(&flat, r#"{}"#).unwrap();
    assert!(mask.iter().all(|&v| v == 0));
    assert_eq!(range, 100.0);
    let sharp = image(|x, y| grey(checker(x, y)));
    let (mask, range) = focus(&sharp, r#"{}"#).unwrap();
    assert_eq!(range, 0.0, "one class takes range 0");
    assert!(
        mask.iter().all(|&v| v >= 250),
        "a sharp picture is in focus everywhere"
    );
}

#[test]
fn malformed_requests_refuse_and_leave_the_outputs_untouched() {
    let img = half_sharp();
    for (json, needle) in [
        (r#"{"bogus":1}"#, "bogus"),
        (r#"{"range":101}"#, "range"),
        (r#"{"range":-1}"#, "range"),
        (r#"{"soften_edge":"yes"}"#, "soften_edge"),
        (r#"[1]"#, ""),
    ] {
        let params = CString::new(json).unwrap();
        let mut mask = vec![9u8; (W * H) as usize];
        let mut range = 9.0;
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            dr_selection_focus_area(
                img.0,
                params.as_ptr(),
                mask.as_mut_ptr(),
                &mut range,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!ok, "{json} was accepted");
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        assert!(message.contains(needle), "{json}: {message}");
        assert!(
            mask.iter().all(|&v| v == 9) && range == 9.0,
            "{json} wrote outputs"
        );
    }
}
