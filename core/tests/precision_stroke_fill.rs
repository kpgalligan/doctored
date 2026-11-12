//! The Stroke's Shape Burst and Dither at 16 bits, at 32 bits and in CMYK
//! ink. The Shape Burst oracle is the phase-3 law
//! (`scripts/photoshop-probe/gradient_identify_673_fit.py`, PHASE-3's
//! `burst.d`: `D` the 8-neighbour chamfer across the contour, Outside `1 −
//! D / (size + 1)`, Inside `D / (size + 1)`, Center one ramp over `size + 2`)
//! under the Classic ramp `S` (`common::classic_ramp`): the 16-bit render
//! within one 8-bit code of `S(t)`, the 32-bit render's linear values (black
//! and white stops, so the linear value IS `S(t)`) within 1e-4, and a CMYK
//! band's inks against the mix by `S(t)` of the inks a Colour Overlay of
//! each stop prints (an independently shipped effect). Dither moves no sample more
//! than one code of the depth it is read at: a u16 code at 16 bits and on
//! 16-bit CMYK, 1/65535 linear at 32 bits, an 8-bit code on 8-bit CMYK. The
//! typed `.rz` claims word 94 right after 46, or right after a CMYK file's
//! rung, both ways; a CMYK PSD carries `shapeburst` and `Dthr`.

use std::ffi::{c_char, CString};
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
use crate::common::{classic_ramp, cpath, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (48, 40);
const RECT: (i32, i32, u32, u32) = (12, 10, 22, 18);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];
const SIZE: f64 = 6.0;
const POSITIONS: [&str; 3] = ["outside", "inside", "center"];

/// One black-to-white gradient stroke with `extra` gradient keys.
fn stroke(position: &str, extra: Value) -> Value {
    let mut gradient = json!({"stops":[
        {"position":0,"color":"#000000","opacity":1},
        {"position":1,"color":"#ffffff","opacity":1}]});
    for (k, v) in extra.as_object().unwrap() {
        gradient[k] = v.clone();
    }
    json!({"effects":[{"type":"stroke","size":SIZE,"position":position,
        "fill_type":"gradient","gradient":gradient}]})
}

fn burst(position: &str) -> Value {
    stroke(position, json!({"style":"shape_burst"}))
}

fn dithered(position: &str) -> Value {
    stroke(position, json!({"style":"shape_burst","dither":true}))
}

/// The 5 x 5 chamfer distance of `(dx, dy)` in closed form (steps 1,
/// sqrt 2, sqrt 5; `style_stroke.rs`'s, restated).
fn chamfer(dx: i64, dy: i64) -> f64 {
    let (a, b) = (dx.abs().max(dy.abs()), dx.abs().min(dy.abs()));
    if a >= 2 * b {
        b as f64 * 5f64.sqrt() + (a - 2 * b) as f64
    } else {
        (a - b) as f64 * 5f64.sqrt() + (2 * b - a) as f64 * 2f64.sqrt()
    }
}

/// The 8-neighbour chamfer distance (steps 1, sqrt 2) of `(dx, dy)` in
/// closed form: `(a − b) + b sqrt 2`, `a >= b` the absolute components.
fn contour(dx: i64, dy: i64) -> f64 {
    let (a, b) = (dx.abs().max(dy.abs()), dx.abs().min(dy.abs()));
    (a - b) as f64 + b as f64 * 2f64.sqrt()
}

/// THE SHAPE BURST law (module doc) at canvas `(x, y)` for a stroke at
/// `position`: `Some(t)` where the band covers the pixel fully, else `None`
/// (the band: outside, the 5 x 5 chamfer D <= reach; inside, the axis
/// distance <= reach). `t` reads the 8-neighbour distance across the
/// contour: outside to the nearest rect pixel, inside the axis distance to
/// the nearest outside pixel (the edge pixel 1).
fn burst_t(position: &str, x: u32, y: u32) -> Option<f64> {
    let (x, y) = (i64::from(x), i64::from(y));
    let (x0, y0) = (i64::from(RECT.0), i64::from(RECT.1));
    let (x1, y1) = (x0 + i64::from(RECT.2), y0 + i64::from(RECT.3));
    let half = SIZE / 2.0;
    let inside = x >= x0 && x < x1 && y >= y0 && y < y1;
    let t = if inside {
        let d = (x - x0 + 1).min(x1 - x).min(y - y0 + 1).min(y1 - y) as f64;
        match position {
            "inside" if d <= SIZE => d / (SIZE + 1.0),
            "center" if d <= half => (d + half) / (SIZE + 2.0),
            _ => return None,
        }
    } else {
        let (dx, dy) = (x - x.clamp(x0, x1 - 1), y - y.clamp(y0, y1 - 1));
        let band = chamfer(dx, dy);
        let d = contour(dx, dy);
        match position {
            "outside" if band <= SIZE => 1.0 - d / (SIZE + 1.0),
            "center" if band <= half => (half + 1.0 - d) / (SIZE + 2.0),
            _ => return None,
        }
    };
    Some(t.clamp(0.0, 1.0))
}

/// The Classic ramp of a black-to-white gradient at `t`
/// (`common::classic_ramp`).
fn ramp(t: f64) -> f64 {
    classic_ramp(&[(0.0, 0.0), (1.0, 1.0)], t)
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

/// A white canvas under the green rect made at RECT, at 16 bits.
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

// ------------------------------------------------------------- 16 bits --

#[test]
fn sixteen_bits_render_shape_burst_within_one_code() {
    for position in POSITIONS {
        let pixels = render16(&set_style(&typed_doc(), &burst(position)));
        let mut checked = 0;
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                let Some(t) = burst_t(position, x, y) else {
                    continue;
                };
                let got = pixels[(y * CANVAS.0 + x) as usize];
                for v in &got[..3] {
                    let e = (f64::from(*v) / 257.0 - ramp(t) * 255.0).abs();
                    assert!(e <= 1.0, "{position} ({x}, {y}): {e} codes");
                }
                checked += 1;
            }
        }
        assert!(checked > 200, "{position}: {checked}");
    }
}

#[test]
fn sixteen_bit_dither_moves_no_sample_more_than_one_u16_code() {
    for position in POSITIONS {
        let plain = render16(&set_style(&typed_doc(), &burst(position)));
        let doc = set_style(&typed_doc(), &dithered(position));
        let (a, b) = (render16(&doc), render16(&doc));
        assert_eq!(a, b, "{position}: deterministic");
        let mut moved = 0;
        for (p, q) in plain.iter().zip(&a) {
            for c in 0..4 {
                let d = (i32::from(p[c]) - i32::from(q[c])).abs();
                assert!(d <= 1, "{position}: a sample moved {d} u16 codes");
                moved += usize::from(d == 1);
            }
        }
        assert!(moved > 0, "{position}: Dither moved no sample");
    }
}

// ------------------------------------------------------------- 32 bits --

fn float_doc() -> PrecisionDocument {
    let float = typed_doc().promote_to_float(LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    float
}

fn render32(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()
}

#[test]
fn thirty_two_bits_read_s_of_t_as_the_linear_value_between_linear_stops() {
    for position in POSITIONS {
        let pixels = render32(&set_style(&float_doc(), &burst(position)));
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                let Some(t) = burst_t(position, x, y) else {
                    continue;
                };
                let got = pixels[(y * CANVAS.0 + x) as usize];
                for c in 0..3 {
                    let e = (f64::from(got[c]) - ramp(t)).abs();
                    assert!(e <= 1e-4, "{position} ({x}, {y}): {got:?} against S({t})");
                }
            }
        }
    }
}

#[test]
fn thirty_two_bit_dither_moves_no_sample_more_than_a_65535th() {
    for position in POSITIONS {
        let plain = render32(&set_style(&float_doc(), &burst(position)));
        let doc = set_style(&float_doc(), &dithered(position));
        let (a, b) = (render32(&doc), render32(&doc));
        assert_eq!(a, b, "{position}: deterministic");
        let mut moved = 0;
        for (p, q) in plain.iter().zip(&a) {
            for c in 0..3 {
                let d = f64::from((p[c] - q[c]).abs());
                assert!(d <= 1.0 / 65535.0 + 1e-6, "{position}: a sample moved {d}");
                moved += usize::from(d > 0.0);
            }
        }
        assert!(moved > 0, "{position}: Dither moved no sample");
    }
}

// ----------------------------------------------------------------- CMYK --

/// A 48 x 40 CMYK document at `depth`, paper under one layer holding an
/// opaque ink rect at RECT.
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

/// The ink a Colour Overlay of `hex` prints inside the rect.
fn overlay_ink(depth: u32, hex: &str) -> [u16; 5] {
    let out = cmyk_flat(
        depth,
        &json!({"effects":[{"type":"color_overlay","color":hex}]}),
    );
    out[((RECT.1 as u32 + 5) * CANVAS.0 + RECT.0 as u32 + 5) as usize]
}

#[test]
fn cmyk_bands_mix_the_colour_overlay_inks_of_the_stops_by_s_of_t() {
    for depth in [8, 16] {
        let (black, white) = (overlay_ink(depth, "#000000"), overlay_ink(depth, "#ffffff"));
        let bound = if depth == 8 { 257.0 } else { 1.0 };
        for position in POSITIONS {
            let got = cmyk_flat(depth, &burst(position));
            let mut checked = 0;
            for y in 0..CANVAS.1 {
                for x in 0..CANVAS.0 {
                    let Some(t) = burst_t(position, x, y) else {
                        continue;
                    };
                    let g = got[(y * CANVAS.0 + x) as usize];
                    for c in 0..4 {
                        let want = f64::from(black[c])
                            + (f64::from(white[c]) - f64::from(black[c])) * ramp(t);
                        assert!(
                            (f64::from(g[c]) - want.round()).abs() <= bound,
                            "{depth}-bit {position} ({x}, {y}) plate {c}: {} against {want}",
                            g[c]
                        );
                    }
                    checked += 1;
                }
            }
            assert!(checked > 200, "{depth}-bit {position}: {checked}");
        }
    }
}

#[test]
fn cmyk_dither_moves_no_plate_more_than_one_code_of_its_depth() {
    for (depth, bound) in [(8, 257), (16, 1)] {
        let plain = cmyk_flat(depth, &burst("center"));
        let a = cmyk_flat(depth, &dithered("center"));
        let b = cmyk_flat(depth, &dithered("center"));
        assert_eq!(a, b, "{depth}-bit: deterministic");
        let mut moved = 0;
        for (p, q) in plain.iter().zip(&a) {
            for c in 0..5 {
                let d = (i32::from(p[c]) - i32::from(q[c])).abs();
                assert!(d <= bound, "{depth}-bit: a plate moved {d}");
                moved += usize::from(d > 0);
            }
        }
        assert!(moved > 0, "{depth}-bit: Dither moved no plate");
    }
}

// --------------------------------------------------------------- .rz 94 --

#[test]
fn the_typed_word_94_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let plain = dir.path().join("plain.rz");
    set_style(&typed_doc(), &stroke("outside", json!({"style":"radial"})))
        .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let plain = std::fs::read(&plain).unwrap();
    assert_ne!(
        &plain[8..12],
        &94u32.to_le_bytes(),
        "no 94 without the options"
    );
    for (name, style) in [
        ("burst", burst("center")),
        ("dither", stroke("inside", json!({"dither":true}))),
    ] {
        let doc = set_style(&typed_doc(), &style);
        let path = dir.path().join(format!("{name}.rz"));
        let path = path.to_str().unwrap();
        doc.save_native(path, LIMIT, 0, None).unwrap();
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
        assert_eq!(&bytes[8..12], &94u32.to_le_bytes(), "{name}: 94 after 46");
        assert_eq!(&bytes[12..16], &plain[8..12], "{name}: then the plain word");
        let reopened = PrecisionDocument::open_native(path, LIMIT, 0, None).unwrap();
        assert_eq!(
            reopened.layer(1).unwrap().reported_style(),
            doc.layer(1).unwrap().reported_style()
        );
        assert_eq!(render16(&reopened), render16(&doc), "{name}");
        let mut stripped = bytes.clone();
        stripped.drain(8..12);
        let cut = dir.path().join(format!("{name}-cut.rz"));
        std::fs::write(&cut, &stripped).unwrap();
        let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None)
        else {
            panic!("{name}: the options without the word must refuse");
        };
        assert_eq!(
            message,
            "native shape burst and dithered gradients require DRDC 94"
        );
    }
    let mut forged = plain.clone();
    forged.splice(8..8, 94u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
    else {
        panic!("the word without the options must refuse");
    };
    assert_eq!(message, "invalid native gradient options word");
}

#[test]
fn a_cmyk_file_claims_word_94_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let doc = styled(cmyk_base(16), 1, &dithered("outside"));
    let path = dir.path().join("cmyk.rz");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    let rung = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert!((82..=88).contains(&rung), "the CMYK rung, got {rung}");
    assert_eq!(&bytes[12..16], &94u32.to_le_bytes(), "94 after the rung");
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
        panic!("CMYK options without the word must refuse");
    };
    assert_eq!(
        message,
        "native shape burst and dithered gradients require DRDC 94"
    );
    free(reopened);
    free(doc);
}

// ------------------------------------------------------------------ PSD --

#[test]
fn a_cmyk_psd_round_trip_carries_shape_burst_and_dither() {
    // `shapeburst` and `Dthr` both ways in a CMYK file (`psd_stroke_fill`
    // holds the 8- and 16-bit RGB round trips; the CMYK helpers live in
    // this suite).
    let dir = TempDir::new().unwrap();
    for depth in [8, 16] {
        let doc = styled(cmyk_base(depth), 1, &dithered("center"));
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
        assert!(
            bytes.windows(10).any(|w| w == b"shapeburst"),
            "{depth}-bit: GrdT shapeburst"
        );
        assert!(
            bytes.windows(9).any(|w| w == b"Dthrbool\x01"),
            "{depth}-bit: Dthr true"
        );
        let back = open_psd(&path, 0).unwrap();
        // The export writes each stop's ink (`CMYC`), which the import keeps
        // as the stop's authoritative ink; the options come back as written.
        let style: Value = serde_json::from_str(
            &unsafe { &*back }
                .layer(1)
                .unwrap()
                .reported_style()
                .unwrap(),
        )
        .unwrap();
        let gradient = &style["effects"][0]["gradient"];
        assert_eq!(gradient["style"], "shape_burst", "{depth}-bit");
        assert_eq!(gradient["dither"], true, "{depth}-bit");
        // The stops now print their written inks, four-decimal percents
        // (`CMYC`): within one code of an 8-bit plate, and of 1e-4 ink
        // (6.6 u16 codes) at 16 bits.
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
