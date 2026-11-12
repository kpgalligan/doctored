//! The Stroke's Fill Type Pattern at 16 bits, at 32 bits and
//! in CMYK ink. The 16-bit render is take1's tile T in closed form × 257
//! (`T(x, y) = (20x + 10, 30y + 15, 200 | 60)` at `((X − Ox) mod 12,
//! (Y − Oy) mod 8)`) within one 8-bit code on the take's judged pixels, and
//! so is Photoshop's own `pt_o_rgb16.tif`; the 32-bit render reads each
//! texel's code / 255 as its LINEAR value; a CMYK band prints the ink a
//! Colour Overlay of the texel's hex prints (an independently shipped
//! effect) at Outside and at Center, at 8 and 16 bits. The typed `.rz`
//! claims word 95 right after 46, or right after a CMYK file's rung, both
//! ways; a CMYK PSD carries the pattern stroke.

use std::collections::HashMap;
use std::ffi::{c_char, CString};
use std::path::PathBuf;
use std::ptr;

use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_psd_import::open_psd;
use crate::cmyk_structure_fixture::free;
use crate::cmyk_style_fixture::{backdrop, flat, hard_rect, layer, styled};
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (13, 9, 50, 40);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];
const SIZE: i32 = 8;

// ---------------------------------------------------------------- helpers --

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

/// take1's tile T: 12 x 8, opaque.
fn texel(x: u32, y: u32) -> [u8; 3] {
    [
        (20 * x + 10) as u8,
        (30 * y + 15) as u8,
        if (x + y).is_multiple_of(2) { 200 } else { 60 },
    ]
}

/// The texel T puts at canvas pixel (x, y), its origin at the canvas origin.
fn want(x: i32, y: i32) -> [u8; 3] {
    texel(x.rem_euclid(12) as u32, y.rem_euclid(8) as u32)
}

fn tile_t() -> Value {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..12 {
            rgba.extend_from_slice(&texel(x, y));
            rgba.push(255);
        }
    }
    json!({"name":"T","width":12,"height":8,"rgba":base64(&rgba)})
}

/// One 8 px linked Pattern stroke of T at `position`, with `extra` fill keys.
fn stroke(position: &str, extra: Value) -> Value {
    let mut fill = json!({"pattern": tile_t()});
    for (k, v) in extra.as_object().unwrap() {
        fill[k] = v.clone();
    }
    json!({"effects":[{"type":"stroke","size":SIZE,"position":position,
        "fill_type":"pattern","pattern_fill":fill}]})
}

/// The take's judged pixels (`stroke_fill_take.py`): the straight runs more
/// than 9 px from every corner, k = 1 .. reach − 1 px from the contour.
fn judged(position: &str) -> Vec<(i32, i32)> {
    let (x0, y0) = (RECT.0, RECT.1);
    let (x1, y1) = (x0 + RECT.2 as i32 - 1, y0 + RECT.3 as i32 - 1);
    let (outside, inside) = match position {
        "outside" => (SIZE - 1, 0),
        "inside" => (0, SIZE - 1),
        _ => (SIZE / 2 - 1, SIZE / 2 - 1),
    };
    let mut out = Vec::new();
    for y in y0 + 10..=y1 - 10 {
        for k in 1..=outside {
            out.extend([(x0 - k, y), (x1 + k, y)]);
        }
        for k in 1..=inside {
            out.extend([(x0 + k - 1, y), (x1 - k + 1, y)]);
        }
    }
    for x in x0 + 10..=x1 - 10 {
        for k in 1..=outside {
            out.extend([(x, y0 - k), (x, y1 + k)]);
        }
        for k in 1..=inside {
            out.extend([(x, y0 + k - 1), (x, y1 - k + 1)]);
        }
    }
    out
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

/// take1's scene at 16 bits: a white canvas under the green rect made at
/// RECT.
fn typed_doc() -> PrecisionDocument {
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Base",
                image16(CANVAS.0, CANVAS.1, [255; 4]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "Rect",
                image16(RECT.2, RECT.3, GREEN),
                (RECT.0, RECT.1),
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn set_style(doc: &PrecisionDocument, style: &Value) -> PrecisionDocument {
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

fn render16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn photoshop_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/stroke_fill")
        .join(name)
}

// ------------------------------------------------------------- 16 bits --

#[test]
fn sixteen_bits_match_photoshops_pt_o_and_the_texels_within_one_code() {
    let ours = render16(&set_style(&typed_doc(), &stroke("outside", json!({}))));
    let path = photoshop_fixture("pt_o_rgb16.tif");
    let theirs = render16(
        &PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap(),
    );
    let pixels = judged("outside");
    assert!(pixels.len() > 300);
    for (x, y) in pixels {
        let i = (y as u32 * CANVAS.0 + x as u32) as usize;
        let w = want(x, y);
        for c in 0..3 {
            let got = i32::from(ours[i][c]);
            assert!(
                (got - i32::from(w[c]) * 257).abs() <= 257,
                "({x}, {y}) channel {c}: {got} against T {}",
                w[c]
            );
            assert!(
                (got - i32::from(theirs[i][c])).abs() <= 257,
                "({x}, {y}) channel {c}: {got} against Photoshop {}",
                theirs[i][c]
            );
        }
    }
}

// ------------------------------------------------------------- 32 bits --

#[test]
fn thirty_two_bits_read_each_texel_as_code_over_255_linear() {
    let float = typed_doc().promote_to_float(LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    for position in ["outside", "inside", "center"] {
        let doc = set_style(&float, &stroke(position, json!({})));
        let pixels = doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap();
        for (x, y) in judged(position) {
            let got = pixels[(y as u32 * CANVAS.0 + x as u32) as usize];
            let w = want(x, y);
            for c in 0..3 {
                let value = f32::from(w[c]) / 255.0;
                assert!(
                    (got[c] - value).abs() <= 1e-4,
                    "{position} ({x}, {y}) channel {c}: {} against code {}",
                    got[c],
                    w[c]
                );
            }
        }
    }
}

// ----------------------------------------------------------------- CMYK --

/// A CMYK document at `depth`, paper under one layer holding an opaque ink
/// rect at RECT.
fn cmyk_base(depth: u32) -> *mut PrecisionDocument {
    let (x0, y0) = (RECT.0 as u32, RECT.1 as u32);
    layer(
        backdrop(CANVAS.0, CANVAS.1, depth, &[[0.0; 4]]),
        "Shape",
        [10.0, 60.0, 30.0, 0.0],
        &hard_rect(CANVAS.0, CANVAS.1, (x0, y0, x0 + RECT.2, y0 + RECT.3)),
    )
}

fn cmyk_flat(depth: u32, style: &Value) -> Vec<[u16; 5]> {
    let doc = styled(cmyk_base(depth), 1, style);
    let out = flat(doc);
    free(doc);
    out
}

#[test]
fn a_cmyk_band_prints_the_colour_overlay_ink_of_each_texel() {
    for depth in [8, 16] {
        let bound = if depth == 8 { 257 } else { 1 };
        // The ink a Colour Overlay of each texel's hex prints inside the rect.
        let mut inks: HashMap<[u8; 3], [u16; 5]> = HashMap::new();
        for position in ["outside", "center"] {
            let got = cmyk_flat(depth, &stroke(position, json!({})));
            for (x, y) in judged(position) {
                let w = want(x, y);
                let ink = *inks.entry(w).or_insert_with(|| {
                    let hex = format!("#{:02x}{:02x}{:02x}", w[0], w[1], w[2]);
                    let out = cmyk_flat(
                        depth,
                        &json!({"effects":[{"type":"color_overlay","color":hex}]}),
                    );
                    out[((RECT.1 as u32 + 20) * CANVAS.0 + RECT.0 as u32 + 20) as usize]
                });
                let g = got[(y as u32 * CANVAS.0 + x as u32) as usize];
                for c in 0..4 {
                    assert!(
                        (i32::from(g[c]) - i32::from(ink[c])).abs() <= bound,
                        "{depth}-bit {position} ({x}, {y}) plate {c}: {g:?} against {ink:?}"
                    );
                }
            }
        }
    }
}

// --------------------------------------------------------------- .rz 95 --

#[test]
fn the_typed_word_95_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let plain = dir.path().join("plain.rz");
    set_style(
        &typed_doc(),
        &json!({"effects":[{"type":"stroke","fill_type":"gradient"}]}),
    )
    .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
    .unwrap();
    let plain = std::fs::read(&plain).unwrap();
    assert_ne!(
        &plain[8..12],
        &95u32.to_le_bytes(),
        "no 95 without a pattern stroke"
    );
    let doc = set_style(&typed_doc(), &stroke("center", json!({"scale": 0.5})));
    let path = dir.path().join("pattern.rz");
    let path = path.to_str().unwrap();
    doc.save_native(path, LIMIT, 0, None).unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &95u32.to_le_bytes(), "95 after 46");
    assert_eq!(&bytes[12..16], &plain[8..12], "then the plain word");
    let reopened = PrecisionDocument::open_native(path, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
    assert_eq!(render16(&reopened), render16(&doc));
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("a pattern stroke without the word must refuse");
    };
    assert_eq!(message, "native pattern strokes require DRDC 95");
    let mut forged = plain.clone();
    forged.splice(8..8, 95u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
    else {
        panic!("the word without a pattern stroke must refuse");
    };
    assert_eq!(message, "invalid native pattern stroke word");
}

#[test]
fn a_cmyk_file_claims_word_95_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let doc = styled(cmyk_base(16), 1, &stroke("outside", json!({})));
    let path = dir.path().join("cmyk.rz");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    let rung = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert!((82..=88).contains(&rung), "the CMYK rung, got {rung}");
    assert_eq!(&bytes[12..16], &95u32.to_le_bytes(), "95 after the rung");
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    let reopened = Box::into_raw(Box::new(reopened));
    assert_eq!(
        unsafe { &*reopened }.layer(1).unwrap().reported_style(),
        unsafe { &*doc }.layer(1).unwrap().reported_style()
    );
    assert_eq!(flat(reopened), flat(doc));
    let mut stripped = bytes.clone();
    stripped.drain(12..16);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("a CMYK pattern stroke without the word must refuse");
    };
    assert_eq!(message, "native pattern strokes require DRDC 95");
    free(reopened);
    free(doc);
}

// ------------------------------------------------------------------ PSD --

#[test]
fn a_cmyk_psd_round_trip_carries_the_pattern_stroke() {
    let dir = TempDir::new().unwrap();
    for depth in [8, 16] {
        let doc = styled(
            cmyk_base(depth),
            1,
            &stroke("center", json!({"phase": [3, 1]})),
        );
        let path = dir.path().join(format!("cmyk{depth}.psd"));
        let mut err = ptr::null_mut();
        let ok = unsafe {
            dr_precision_doc_save_psd(
                doc,
                ptr::null(),
                cpath(&path).as_ptr(),
                0,
                0,
                LIMIT,
                0,
                std::ptr::null(),
                &mut err,
            )
        };
        assert!(ok, "{}", take_err_string(err));
        let bytes = std::fs::read(&path).unwrap();
        let key: &[u8] = if depth == 8 { b"8BIMPatt" } else { b"8BIMPat2" };
        assert!(
            bytes.windows(8).any(|w| w == key),
            "{depth}-bit: the pattern table"
        );
        assert!(bytes.windows(4).any(|w| w == b"Lnkd"), "{depth}-bit: Lnkd");
        let back = open_psd(&path, 0).unwrap();
        let reported = |d: *mut PrecisionDocument| -> Value {
            serde_json::from_str(&unsafe { &*d }.layer(1).unwrap().reported_style().unwrap())
                .unwrap()
        };
        let (theirs, ours) = (reported(back), reported(doc));
        assert_eq!(theirs["effects"][0]["fill_type"], "pattern", "{depth}-bit");
        assert_eq!(
            theirs["effects"][0]["pattern_fill"], ours["effects"][0]["pattern_fill"],
            "{depth}-bit"
        );
        // The layer's own ink crosses as written plates (four-decimal
        // percents): within one code of an 8-bit plate, and of 1e-4 ink
        // (6.6 u16 codes) at 16 bits; the band prints the same tile.
        let bound = if depth == 8 { 257 } else { 7 };
        let worst = flat(back)
            .iter()
            .zip(flat(doc))
            .flat_map(|(a, b)| (0..5).map(move |c| (i32::from(a[c]) - i32::from(b[c])).abs()))
            .max()
            .unwrap();
        assert!(worst <= bound, "{depth}-bit: a plate moved {worst}");
        free(back);
        free(doc);
    }
}
