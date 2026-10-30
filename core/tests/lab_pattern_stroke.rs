//! #547 Lab phase 2f, second part, through the FFI: a Pattern Overlay and a
//! Pattern stroke on a Lab Color layer, and Edit ▸ Stroke… of a layer edge.
//! The oracle is the hand-computed CIE L\*a\*b\* of each texel's sRGB
//! (`lab_fixture::srgb_to_lab`), the tile being authored sRGB on a Lab
//! document; the bound is the 8-bit Lab bound (0.6 L\*, 1.0 a\*/b\*), since
//! each texel becomes 8-bit Lab codes. A 2 × 2 red, blue, green and white
//! tile at scale 1 lands on each texel's L\*a\*b\* at 8 and 16 bits, every
//! code on the ×257 grid and alpha opaque; a red and green tile at scale 0.5
//! reads the mean of the texels' L\*a\*b\* (which differs from the
//! L\*a\*b\* of their mean sRGB by more than 2 units, so the texels convert
//! before the pyramid); the checkerboard preset reads white's and
//! #c0c0c0's L\* with a\* and b\* near 0; a Pattern stroke's band reads its
//! texels; RGB → Lab and back carry a pattern style byte-equal; a layer
//! edge's red Stroke… equals the same request on an sRGB document holding
//! the same codes with the colour as `new_lab`'s red codes (at 8 bits on
//! the ×257 grid), within 1 code; Darken refuses and Hue is admitted
//! (phase 3). Bounds declared before measuring.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_lab::{dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_new_lab};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{edit, free, result, LIMIT};
use crate::lab_fixture::{decode, srgb_to_lab};
use crate::lab_targets::{codes, pattern, srgb_doc, to_lab, H, W};

/// The 8-bit Lab bound: L\*, then a\* and b\*.
const LAB_BOUND: (f64, f64) = (0.6, 1.0);

/// The bound the scale-0.5 mean must differ from the mean sRGB's L\*a\*b\*
/// by, in the largest channel.
const PYRAMID_GAP: f64 = 2.0;

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

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

/// A 2 × 2 opaque tile: `texels` in row order.
fn tile(texels: [[u8; 3]; 4]) -> Value {
    let mut rgba = Vec::new();
    for t in texels {
        rgba.extend_from_slice(&t);
        rgba.push(255);
    }
    json!({"name":"T","width":2,"height":2,"rgba":base64(&rgba)})
}

/// (a)'s tile: red, blue / green, white.
fn four() -> [[u8; 3]; 4] {
    [RED, BLUE, GREEN, WHITE]
}

/// The texel of a 2 × 2 canvas-aligned tile at canvas pixel (x, y).
fn texel_at(texels: [[u8; 3]; 4], x: u32, y: u32) -> [u8; 3] {
    texels[((y % 2) * 2 + x % 2) as usize]
}

fn unit(rgb: [u8; 3]) -> [f64; 3] {
    rgb.map(|v| f64::from(v) / 255.0)
}

/// A white `w` × `h` Lab document of `depth` (File ▸ New ▸ Lab Color).
fn new_lab(w: u32, h: u32, depth: u32, fill: [f64; 3]) -> *mut PrecisionDocument {
    let request = json!({"depth":depth,"fill_srgb":[fill[0],fill[1],fill[2],1]}).to_string();
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(w, h, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn styled(doc: *const PrecisionDocument, effect: Value) -> *mut PrecisionDocument {
    let request = json!({"op":"layer_style","version":1,"layer":0,"style":{"effects":[effect]}});
    edit(doc, &request, &[])
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .expect("the style changed nothing")
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn style_json(doc: *const PrecisionDocument) -> Option<String> {
    unsafe { &*doc }.layer(0).unwrap().reported_style()
}

/// `p` decodes within [`LAB_BOUND`] of the L\*a\*b\* `want`.
fn assert_near(p: [u16; 4], want: [f64; 3], what: &str) {
    let got = decode(p);
    let bounds = [LAB_BOUND.0, LAB_BOUND.1, LAB_BOUND.1];
    for c in 0..3 {
        assert!(
            (got[c] - want[c]).abs() <= bounds[c],
            "{what} channel {c}: {got:?} against {want:?}"
        );
    }
}

#[test]
fn a_custom_tile_lands_on_each_texels_lab() {
    let (w, h) = (8, 6);
    for depth in [8, 16] {
        let lab = new_lab(w, h, depth, [1.0, 1.0, 1.0]);
        let out = styled(
            lab,
            json!({"type":"pattern_overlay","pattern":tile(four()),"scale":1}),
        );
        let px = render(out);
        for (i, p) in px.iter().enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            let what = format!("{depth}-bit pixel ({x}, {y})");
            assert_near(*p, srgb_to_lab(unit(texel_at(four(), x, y))), &what);
            for c in 0..3 {
                assert_eq!(p[c] % 257, 0, "{what} channel {c}: {p:?}");
            }
            assert_eq!(p[3], 65535, "{what}");
        }
        free(out);
        free(lab);
    }
}

#[test]
fn half_scale_boxes_the_lab_values_of_the_texels() {
    let texels = [RED, GREEN, GREEN, RED];
    let (r, g) = (srgb_to_lab(unit(RED)), srgb_to_lab(unit(GREEN)));
    let mean: [f64; 3] = std::array::from_fn(|c| (r[c] + g[c]) / 2.0);
    let of_mean = srgb_to_lab([0.5, 0.5, 0.0]);
    let gap = (0..3)
        .map(|c| (mean[c] - of_mean[c]).abs())
        .fold(0.0, f64::max);
    assert!(gap > PYRAMID_GAP, "{mean:?} against {of_mean:?}");
    let (w, h) = (8, 6);
    let lab = new_lab(w, h, 16, [1.0, 1.0, 1.0]);
    let out = styled(
        lab,
        json!({"type":"pattern_overlay","pattern":tile(texels),"scale":0.5}),
    );
    for (i, p) in render(out).iter().enumerate() {
        assert_near(*p, mean, &format!("pixel {i}"));
    }
    free(out);
    free(lab);
}

#[test]
fn the_checkerboard_preset_reads_white_and_c0_lightness() {
    let (w, h) = (32, 24);
    let lab = new_lab(w, h, 16, [1.0, 1.0, 1.0]);
    let out = styled(
        lab,
        json!({"type":"pattern_overlay","pattern":{"preset":"checkerboard"}}),
    );
    let light = srgb_to_lab([1.0; 3]);
    let silver = srgb_to_lab([f64::from(0xc0u8) / 255.0; 3]);
    let mut seen = [false; 2];
    for (i, p) in render(out).iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let white = (x / 8 + y / 8) % 2 == 0;
        seen[usize::from(white)] = true;
        let want = if white { light } else { silver };
        let got = decode(*p);
        assert!(
            (got[0] - want[0]).abs() <= LAB_BOUND.0,
            "pixel ({x}, {y}): L* {} against {}",
            got[0],
            want[0]
        );
        for c in 1..3 {
            assert!(got[c].abs() <= LAB_BOUND.1, "pixel ({x}, {y}): {got:?}");
        }
    }
    assert_eq!(seen, [true, true]);
    free(out);
    free(lab);
}

#[test]
fn a_pattern_strokes_band_reads_its_texels() {
    // An opaque 8 × 6 layer at (4, 3) on a 16 × 12 canvas.
    let (cw, ch, ox, oy, lw, lh) = (16u32, 12u32, 4u32, 3u32, 8u32, 6u32);
    let image = PrecisionImage::from_rgba16(
        lw,
        lh,
        vec![[30000, 30000, 30000, 65535]; (lw * lh) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let layer =
        PrecisionDocumentLayer::raster("Layer", image, (ox as i32, oy as i32), 1.0, true).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            cw,
            ch,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .unwrap(),
    ));
    let lab = to_lab(rgb, r#"{"depth":16}"#).unwrap();
    free(rgb);
    let out = styled(
        lab,
        json!({"type":"stroke","size":2,"position":"inside","fill_type":"pattern",
            "pattern_fill":{"pattern":tile(four())}}),
    );
    let px = render(out);
    // The outermost ring's straight runs, 2 px clear of every corner.
    let mut band = Vec::new();
    for x in ox + 2..ox + lw - 2 {
        band.extend([(x, oy), (x, oy + lh - 1)]);
    }
    for y in oy + 2..oy + lh - 2 {
        band.extend([(ox, y), (ox + lw - 1, y)]);
    }
    for (x, y) in band {
        let p = px[(y * cw + x) as usize];
        let what = format!("band pixel ({x}, {y})");
        assert_near(p, srgb_to_lab(unit(texel_at(four(), x, y))), &what);
    }
    free(out);
    free(lab);
}

fn to_srgb(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let icc = srgb.bytes();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            doc,
            icc.as_ptr(),
            icc.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).unwrap().expect("to RGB answered nothing")
}

#[test]
fn rgb_to_lab_and_back_carry_a_pattern_style_byte_equal() {
    let rgb = srgb_doc(pattern());
    let patterned = styled(
        rgb,
        json!({"type":"pattern_overlay","pattern":tile(four()),"scale":0.5,"angle":30}),
    );
    let style = style_json(patterned);
    assert!(style.is_some());
    let lab = to_lab(patterned, "{}").unwrap();
    assert!(unsafe { &*lab }.is_lab());
    assert_eq!(style_json(lab), style, "RGB → Lab");
    let back = to_srgb(lab);
    assert!(!unsafe { &*back }.is_lab());
    assert_eq!(style_json(back), style, "Lab → RGB");
    for doc in [back, lab, patterned, rgb] {
        free(doc);
    }
}

/// The edge stroke request (`stroke_layer_edge`), outside, 2 px.
fn edge_stroke(color: [f64; 4], space: &str, mode: u32) -> Value {
    json!({"op":"stroke_layer_edge","layer":0,"width":2.0,"location":"outside","color":color,
        "color_space":space,"opacity":1.0,"blend_mode":mode})
}

/// The pattern with a transparent border 8 px wide and 6 px tall.
fn bordered() -> Vec<[u16; 4]> {
    pattern()
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let (x, y) = (i as u32 % W, i as u32 / W);
            let inside = (8..W - 8).contains(&x) && (6..H - 6).contains(&y);
            if inside {
                p
            } else {
                [p[0], p[1], p[2], 0]
            }
        })
        .collect()
}

#[test]
fn a_layer_edge_stroke_equals_the_srgb_twin_on_the_lab_codes() {
    for depth in [8u32, 16] {
        let rgb = srgb_doc(bordered());
        let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
        free(rgb);
        let twin = srgb_doc(codes(lab, 0));
        let red_doc = new_lab(1, 1, depth, [1.0, 0.0, 0.0]);
        let red = codes(red_doc, 0)[0];
        free(red_doc);
        if depth == 8 {
            assert!(red.iter().all(|v| v % 257 == 0), "{red:?}");
        }
        let ink = [0, 1, 2].map(|c| f64::from(red[c]) / 65535.0);
        let what = format!("{depth}-bit stroke");
        let a = edit(lab, &edge_stroke([1.0, 0.0, 0.0, 1.0], "srgb", 0), &[])
            .unwrap_or_else(|e| panic!("{what}: {e}"))
            .expect("the stroke changed nothing");
        let b = edit(
            twin,
            &edge_stroke([ink[0], ink[1], ink[2], 1.0], "document", 0),
            &[],
        )
        .unwrap_or_else(|e| panic!("{what} twin: {e}"))
        .expect("the twin's stroke changed nothing");
        let (out, twin_out) = (codes(a, 0), codes(b, 0));
        assert_ne!(out, codes(lab, 0), "{what} changed nothing");
        assert_eq!(out.len(), twin_out.len(), "{what}");
        let tolerance: i32 = 1;
        for (i, (p, q)) in out.iter().zip(&twin_out).enumerate() {
            for c in 0..4 {
                assert!(
                    (i32::from(p[c]) - i32::from(q[c])).abs() <= tolerance,
                    "{what} pixel {i} channel {c}: {p:?} against {q:?}"
                );
            }
        }
        assert!(unsafe { &*a }.is_lab());
        for d in [a, b, twin, lab] {
            free(d);
        }
    }
}

#[test]
fn a_layer_edge_stroke_refuses_darken_and_admits_hue() {
    let rgb = srgb_doc(bordered());
    let lab = to_lab(rgb, "{}").unwrap();
    free(rgb);
    let red = [1.0, 0.0, 0.0, 1.0];
    assert_eq!(
        edit(lab, &edge_stroke(red, "srgb", 6), &[]).unwrap_err(),
        "The Darken blend mode is unavailable in a Lab Color document."
    );
    // Phase 3: Hue composites by its Lab arm.
    let hue = edit(lab, &edge_stroke(red, "srgb", 23), &[])
        .unwrap()
        .expect("the Hue stroke changed nothing");
    assert!(unsafe { &*hue }.is_lab());
    free(hue);
    free(lab);
}
