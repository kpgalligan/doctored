//! Pattern Overlay in a CMYK document: each texel converts to ink
//! through the leg a Colour Overlay's colour takes. The ORACLE is the
//! Colour Overlay itself — an independently shipped effect: at scale 1 every
//! probe on a texel centre equals a Colour Overlay of that texel's hex on
//! the same layer, and at 50 % a 1-px checker (one halving) equals the mean
//! of the two overlays' inks, at 8 and 16 bits. Convert to CMYK carries the
//! style, the typed `.rz` claims it with word 90 right after the CMYK rung,
//! and a layered PSD round trip renders the same.

use std::ffi::c_char;
use std::path::Path;
use std::ptr;

use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_psd_import::open_psd;
use crate::cmyk_structure_fixture::{free, LIMIT};
use crate::cmyk_style_fixture::{backdrop, flat, hard_rect, layer, styled};
use crate::common::{cpath, take_err_string};

const W: u32 = 12;
const H: u32 = 6;

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// A custom tile of `w × h` opaque texels `f(x, y)`.
fn tile(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Value {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&f(x, y));
            rgba.push(255);
        }
    }
    json!({"name":"Probe","width":w,"height":h,"rgba":base64(&rgba)})
}

/// The 4 × 3 probe tile: twelve distinct colours.
fn probe(x: u32, y: u32) -> [u8; 3] {
    [
        (40 + 50 * x) as u8,
        (30 + 70 * y) as u8,
        (200 - 40 * x - 20 * y) as u8,
    ]
}

fn pattern(tile: Value, scale: f64) -> Value {
    json!({"effects":[{"type":"pattern_overlay","pattern":tile,"scale":scale}]})
}

fn color_overlay(rgb: [u8; 3]) -> Value {
    let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    json!({"effects":[{"type":"color_overlay","color":hex}]})
}

/// A `W × H` CMYK document at `depth` with one opaque full-canvas layer.
fn base(depth: u32) -> *mut PrecisionDocument {
    layer(
        backdrop(W, H, depth, &[[0.0; 4]]),
        "Shape",
        [10.0, 60.0, 30.0, 0.0],
        &hard_rect(W, H, (0, 0, W, H)),
    )
}

/// The flattened composite of `base(depth)` carrying `style` on its layer.
fn flat_with(depth: u32, style: &Value) -> Vec<[u16; 5]> {
    let doc = styled(base(depth), 1, style);
    let out = flat(doc);
    free(doc);
    out
}

/// One 8-bit code is 257 sixteen-bit codes.
fn bound(depth: u32) -> u16 {
    if depth == 8 {
        257
    } else {
        1
    }
}

fn assert_ink(got: [u16; 5], want: [f64; 4], bound: u16, what: &str) {
    for c in 0..4 {
        let w = want[c].round() as i64;
        assert!(
            (i64::from(got[c]) - w).unsigned_abs() <= u64::from(bound),
            "{what}: plate {c}: got {got:?}, want {want:?}"
        );
    }
}

#[test]
fn every_probe_is_the_colour_overlay_of_its_texel() {
    for depth in [8, 16] {
        let patterned = flat_with(depth, &pattern(tile(4, 3, probe), 1.0));
        for y in 0..3 {
            for x in 0..4 {
                let overlay = flat_with(depth, &color_overlay(probe(x, y)));
                // The texel and its next period: the tile wraps.
                for (px, py) in [(x, y), (x + 4, y + 3)] {
                    let i = (py * W + px) as usize;
                    let want = [0, 1, 2, 3].map(|c| f64::from(overlay[i][c]));
                    assert_ink(
                        patterned[i],
                        want,
                        bound(depth),
                        &format!("{depth}-bit probe ({px}, {py})"),
                    );
                }
            }
        }
    }
}

#[test]
fn a_halved_checker_is_the_mean_of_its_two_inks() {
    let (a, b) = ([230u8, 40, 20], [20u8, 90, 210]);
    for depth in [8, 16] {
        let checker = tile(2, 2, |x, y| if (x + y).is_multiple_of(2) { a } else { b });
        let patterned = flat_with(depth, &pattern(checker, 0.5));
        let (ia, ib) = (
            flat_with(depth, &color_overlay(a)),
            flat_with(depth, &color_overlay(b)),
        );
        for i in [0usize, 7, 30, 71] {
            let want = [0, 1, 2, 3].map(|c| (f64::from(ia[i][c]) + f64::from(ib[i][c])) / 2.0);
            assert_ink(
                patterned[i],
                want,
                bound(depth),
                &format!("{depth}-bit checker at {i}"),
            );
        }
    }
}

fn image16(w: u32, h: u32, px: [u8; 4]) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        w,
        h,
        vec![px.map(|v| u16::from(v) * 257); (w * h) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

fn reported(doc: *const PrecisionDocument, layer: usize) -> Value {
    let text = unsafe { &*doc }.layer(layer).unwrap().reported_style();
    text.map_or(Value::Null, |t| serde_json::from_str(&t).unwrap())
}

#[test]
fn convert_to_cmyk_carries_the_style() {
    let rgb = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Base", image16(W, H, [255; 4]), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Rect", image16(8, 4, [48, 160, 80, 255]), (2, 1), 1.0, true).unwrap(),
        ],
    )
    .unwrap();
    let rgb = Box::into_raw(Box::new(rgb));
    let rgb = styled(rgb, 1, &pattern(tile(4, 3, probe), 1.0));
    let profile = press_profile("Press Test");
    let mut err: *mut c_char = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "{}", take_err_string(err));
    assert!(unsafe { &*cmyk }.cmyk_policy().is_some());
    let before = reported(rgb, 1);
    let after = reported(cmyk, 1);
    assert_eq!(after["effects"], before["effects"], "the overlay carries");
    assert_eq!(after["effects"][0]["anchor"], json!([2, 1]));
    // It renders: the linked tile starts at the canvas origin, so the
    // rect's pixel (2, 1) is texel (2, 1), the Colour Overlay of its hex.
    let patterned = flat(cmyk);
    let overlay = styled(cmyk, 1, &color_overlay(probe(2, 1)));
    let o = flat(overlay);
    let i = (W + 2) as usize;
    let want = [0, 1, 2, 3].map(|c| f64::from(o[i][c]));
    assert_ink(patterned[i], want, 1, "converted probe (2, 1)");
    free(overlay);
    free(rgb);
}

#[test]
fn the_native_file_claims_word_90_after_the_rung_and_reopens() {
    let dir = TempDir::new().unwrap();
    let doc = styled(base(16), 1, &pattern(tile(4, 3, probe), 0.37));
    let path = dir.path().join("cmyk.rz");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    let rung = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert!((82..=88).contains(&rung), "the CMYK rung, got {rung}");
    assert_eq!(
        &bytes[12..16],
        &90u32.to_le_bytes(),
        "word 90 after the rung"
    );
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    let reopened = Box::into_raw(Box::new(reopened));
    assert_eq!(reported(reopened, 1), reported(doc, 1));
    assert_eq!(flat(reopened), flat(doc));
    free(reopened);
    free(doc);
}

fn save_layers(doc: *const PrecisionDocument, path: &Path) {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
}

#[test]
fn a_layered_psd_round_trip_renders_the_same() {
    let dir = TempDir::new().unwrap();
    for (depth, key) in [(8, b"8BIMPatt"), (16, b"8BIMPat2")] {
        let doc = styled(base(depth), 1, &pattern(tile(4, 3, probe), 0.5));
        let path = dir.path().join(format!("cmyk{depth}.psd"));
        save_layers(doc, &path);
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            bytes.windows(8).any(|w| w == key),
            "{depth}-bit pattern table"
        );
        let back = open_psd(&path, 0).unwrap();
        let carried = reported(back, 1);
        assert_eq!(carried["effects"], reported(doc, 1)["effects"]);
        assert_eq!(flat(back), flat(doc), "{depth}-bit");
        free(back);
        free(doc);
    }
}
