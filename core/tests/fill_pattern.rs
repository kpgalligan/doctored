//! Edit ▸ Fill… with Pattern contents, through the FFI: the 8-bit
//! `dr_doc_fill_selection_pattern` and the typed `paint_blend` with a
//! `pattern` at 16 and 32 bits and in Grayscale, Lab, CMYK, Indexed and
//! Bitmap; and Define Pattern's float row, `dr_pattern_tile_from_linear`.
//!
//! The oracle is the placement rule written from its definition alone:
//! canvas pixel (x, y) takes texel `tile[(y mod h)·w + (x mod w)]`, whatever
//! the layer's offset, its alpha multiplying the opacity and the coverage;
//! then the suite's reference composite (`common::ref_composite`), or at 16
//! bits a Normal over in f64. A mode whose reading of a texel is an existing
//! conversion (Lab, CMYK, Indexed, Bitmap) is checked against the same fill
//! with a 1 × 1 tile of that texel: the w × h fill equals, pixel by pixel,
//! the one-texel fill of the texel the rule names.

use std::ffi::{c_int, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_string_free};
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_fill_stroke::dr_doc_fill_selection_pattern;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::ffi_style::dr_pattern_tile_from_linear;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::bitmap_mode::{bits, gray_doc, to_bitmap};
use crate::cmyk_structure_fixture::{cmyk_layers, RgbLayer};
use crate::common::{
    img_dims, img_pixels, layer_dims, layer_pixels, pixel, ref_composite, solid, take_err_string,
    to_unit, BLEND_MULTIPLY, BLEND_NORMAL, BLEND_PASS_THROUGH,
};
use crate::float_fixture::float_doc;
use crate::indexed_fixture::{free, indices, layer16, paint, rgb_doc, to_indexed, LIMIT};

/// A 3 × 2 tile of distinct opaque colours.
const OPAQUE: [[u8; 4]; 6] = [
    [200, 30, 40, 255],
    [10, 220, 90, 255],
    [60, 70, 230, 255],
    [250, 250, 20, 255],
    [0, 0, 0, 255],
    [128, 128, 128, 255],
];

/// A 3 × 2 tile whose alphas run from opaque to clear.
const TRANSLUCENT: [[u8; 4]; 6] = [
    [200, 30, 40, 255],
    [10, 220, 90, 128],
    [60, 70, 230, 64],
    [250, 250, 20, 0],
    [0, 0, 0, 200],
    [128, 128, 128, 30],
];

const TW: u32 = 3;
const TH: u32 = 2;

/// The rule (module doc): the texel canvas pixel (x, y) takes.
fn texel(tile: &[[u8; 4]], w: u32, h: u32, x: i64, y: i64) -> [u8; 4] {
    let tx = x.rem_euclid(i64::from(w)) as usize;
    let ty = y.rem_euclid(i64::from(h)) as usize;
    tile[ty * w as usize + tx]
}

fn flat(tile: &[[u8; 4]]) -> Vec<u8> {
    tile.iter().flatten().copied().collect()
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The `pattern` object a request carries for `tile`.
fn pattern(tile: &[[u8; 4]], w: u32, h: u32) -> Value {
    json!({"name": "Test", "width": w, "height": h, "rgba": base64(&flat(tile))})
}

#[allow(clippy::too_many_arguments)]
fn byte_fill(
    doc: &DrDocument,
    idx: usize,
    mask: Option<&[u8]>,
    tile: &[[u8; 4]],
    (tw, th): (u32, u32),
    mode: c_int,
    opacity: f32,
    preserve: bool,
) -> Option<*mut DrDocument> {
    let bytes = flat(tile);
    let out = unsafe {
        dr_doc_fill_selection_pattern(
            doc,
            idx,
            mask.map_or(ptr::null(), |m| m.as_ptr()),
            doc.width,
            doc.height,
            bytes.as_ptr(),
            tw,
            th,
            mode,
            opacity,
            preserve,
        )
    };
    (!out.is_null()).then_some(out)
}

#[test]
fn byte_fill_repeats_the_tile_from_the_canvas_origin() {
    let doc = DrDocument::from_pixels(solid(10, 7, [255, 255, 255, 255]));
    let out = byte_fill(&doc, 0, None, &OPAQUE, (TW, TH), BLEND_NORMAL, 1.0, false).unwrap();
    let px = layer_pixels(out, 0);
    for y in 0..7 {
        for x in 0..10 {
            let want = texel(&OPAQUE, TW, TH, i64::from(x), i64::from(y));
            assert_eq!(pixel(&px, 10, x, y), want, "({x}, {y})");
        }
    }
    unsafe { dr_doc_free(out) };
}

#[test]
fn byte_fill_mixes_translucent_texels_through_the_mask_and_opacity() {
    let base = image::RgbaImage::from_fn(8, 5, |x, y| {
        let i = y * 8 + x;
        image::Rgba([(i * 37 % 256) as u8, (i * 11 % 256) as u8, 180, 255])
    });
    let doc = DrDocument::from_pixels(base.clone());
    let mask: Vec<u8> = (0..40u32).map(|i| (i * 53 % 256) as u8).collect();
    for mode in [BLEND_NORMAL, BLEND_MULTIPLY] {
        let out = byte_fill(
            &doc,
            0,
            Some(&mask),
            &TRANSLUCENT,
            (TW, TH),
            mode,
            0.6,
            false,
        )
        .unwrap();
        let px = layer_pixels(out, 0);
        for y in 0..5u32 {
            for x in 0..8u32 {
                let t = texel(&TRANSLUCENT, TW, TH, i64::from(x), i64::from(y));
                let coverage = u32::from(mask[(y * 8 + x) as usize]);
                // The texel's alpha times the coverage, as the walk's byte.
                let alpha = (u32::from(t[3]) * coverage + 127) / 255;
                let src = [t[0], t[1], t[2], alpha as u8];
                let bg = base.get_pixel(x, y).0;
                let want = ref_composite(to_unit(bg), to_unit(src), 0.6, mode);
                let got = pixel(&px, 8, x, y);
                for c in 0..4 {
                    let w = (want[c] * 255.0).round() as i32;
                    assert!(
                        (i32::from(got[c]) - w).abs() <= 1,
                        "mode {mode} ({x}, {y}) channel {c}: {got:?} vs {want:?}"
                    );
                }
            }
        }
        unsafe { dr_doc_free(out) };
    }
}

#[test]
fn byte_fill_keeps_the_canvas_grid_on_an_offset_layer() {
    let doc = DrDocument::from_pixels(solid(10, 7, [255, 255, 255, 255]))
        .adding_image_layer(0, solid(4, 3, [0, 0, 255, 255]), "Offset")
        .unwrap();
    // An offset that is no multiple of the 3 × 2 period, so a tile anchored
    // at the layer would read other texels.
    let doc = doc.with_layer_offset(1, 4, 1).unwrap();
    let out = byte_fill(&doc, 1, None, &OPAQUE, (TW, TH), BLEND_NORMAL, 1.0, false).unwrap();
    assert_eq!(layer_dims(out, 1), (4, 3), "the layer never grows");
    let px = layer_pixels(out, 1);
    for ly in 0..3 {
        for lx in 0..4 {
            let want = texel(&OPAQUE, TW, TH, i64::from(lx) + 4, i64::from(ly) + 1);
            assert_eq!(pixel(&px, 4, lx, ly), want, "layer ({lx}, {ly})");
        }
    }
    unsafe { dr_doc_free(out) };
}

#[test]
fn byte_fill_under_preserve_transparency_keeps_every_alpha() {
    let layer = image::RgbaImage::from_fn(6, 4, |x, y| {
        image::Rgba([90, 90, 90, [0u8, 255, 100, 30][((x + y) % 4) as usize]])
    });
    let doc = DrDocument::from_pixels(solid(6, 4, [255, 255, 255, 255]))
        .adding_image_layer(0, layer.clone(), "Layer")
        .unwrap();
    let out = byte_fill(&doc, 1, None, &OPAQUE, (TW, TH), BLEND_NORMAL, 1.0, true).unwrap();
    let px = layer_pixels(out, 1);
    for (x, y, before) in layer.enumerate_pixels() {
        let got = pixel(&px, 6, x, y);
        assert_eq!(got[3], before.0[3], "alpha at ({x}, {y})");
        if before.0[3] == 0 {
            assert_eq!(got, before.0, "an alpha-0 pixel stays");
        }
    }
    unsafe { dr_doc_free(out) };
}

#[test]
fn byte_fill_refusals() {
    let doc = DrDocument::from_pixels(solid(4, 4, [255, 255, 255, 255]));
    let big = vec![[1u8, 2, 3, 255]; 513];
    assert!(byte_fill(&doc, 0, None, &big, (513, 1), BLEND_NORMAL, 1.0, false).is_none());
    assert!(byte_fill(&doc, 0, None, &OPAQUE, (0, 2), BLEND_NORMAL, 1.0, false).is_none());
    assert!(byte_fill(
        &doc,
        0,
        None,
        &OPAQUE,
        (TW, TH),
        BLEND_PASS_THROUGH,
        1.0,
        false
    )
    .is_none());
    assert!(byte_fill(&doc, 3, None, &OPAQUE, (TW, TH), BLEND_NORMAL, 1.0, false).is_none());
    let out = unsafe {
        dr_doc_fill_selection_pattern(&doc, 0, ptr::null(), 4, 4, ptr::null(), 1, 1, 0, 1.0, false)
    };
    assert!(out.is_null(), "a NULL tile refuses");
    let wrong = unsafe {
        let bytes = flat(&OPAQUE);
        dr_doc_fill_selection_pattern(
            &doc,
            0,
            ptr::null(),
            5,
            4,
            bytes.as_ptr(),
            3,
            2,
            0,
            1.0,
            false,
        )
    };
    assert!(
        wrong.is_null(),
        "a mask size that is not the canvas refuses"
    );
    // The same tile over itself changes nothing: no phantom step.
    let once = byte_fill(&doc, 0, None, &OPAQUE, (TW, TH), BLEND_NORMAL, 1.0, false).unwrap();
    let again = byte_fill(
        unsafe { &*once },
        0,
        None,
        &OPAQUE,
        (TW, TH),
        BLEND_NORMAL,
        1.0,
        false,
    );
    assert!(again.is_none());
    unsafe { dr_doc_free(once) };
}

/// The typed `paint_blend` request with `tile`; `extra`'s keys replace the
/// defaults, a null one removing its key (a CMYK request's colour).
fn pattern_request(tile: &Value, extra: Value) -> Value {
    let mut request = json!({"op": "paint_blend", "layer": 0, "color": [0, 0, 0, 1],
        "color_space": "document", "opacity": 1.0, "blend_mode": 0, "pattern": tile});
    let object = request.as_object_mut().unwrap();
    for (key, value) in extra.as_object().unwrap() {
        if value.is_null() {
            object.remove(key);
        } else {
            object.insert(key.clone(), value.clone());
        }
    }
    request
}

fn full(doc: *const PrecisionDocument) -> Vec<u16> {
    let (w, h) = unsafe { &*doc }.dimensions();
    vec![65535; (w * h) as usize]
}

/// `doc` filled with `request` over full coverage: the new document.
fn typed_fill(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    paint(doc, request, &full(doc))
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .expect("the fill changed something")
}

#[test]
fn u16_fill_reads_each_texel_times_257_and_mixes_its_alpha() {
    let base: Vec<[u8; 4]> = (0..35u32).map(|i| [(i * 7) as u8, 40, 200, 255]).collect();
    let doc = rgb_doc(7, 5, &base);
    let opaque = typed_fill(doc, &pattern_request(&pattern(&OPAQUE, TW, TH), json!({})));
    for (i, p) in layer16(opaque).iter().enumerate() {
        let (x, y) = ((i % 7) as i64, (i / 7) as i64);
        let t = texel(&OPAQUE, TW, TH, x, y);
        assert_eq!(*p, t.map(|c| u16::from(c) * 257), "({x}, {y})");
    }
    let mixed = typed_fill(
        doc,
        &pattern_request(&pattern(&TRANSLUCENT, TW, TH), json!({"opacity": 0.5})),
    );
    for (i, p) in layer16(mixed).iter().enumerate() {
        let (x, y) = ((i % 7) as i64, (i / 7) as i64);
        let t = texel(&TRANSLUCENT, TW, TH, x, y);
        let sa = f64::from(t[3]) / 255.0 * 0.5;
        for c in 0..3 {
            let want = f64::from(base[i][c]) * 257.0 * (1.0 - sa) + f64::from(t[c]) * 257.0 * sa;
            assert!(
                (f64::from(p[c]) - want).abs() <= 1.0,
                "({x}, {y}) channel {c}: {} vs {want}",
                p[c]
            );
        }
        assert_eq!(p[3], 65535);
    }
    free(opaque);
    free(mixed);
    free(doc);
}

#[test]
fn a_tile_past_64_kib_of_request_is_admitted() {
    // 160² texels are 136,536 bytes of base64: past the common 64 KiB bound.
    let side = 160u32;
    let tile: Vec<[u8; 4]> = (0..side * side)
        .map(|i| [(i % 251) as u8, (i / 160 % 256) as u8, 77, 255])
        .collect();
    let doc = rgb_doc(165, 162, &vec![[255, 255, 255, 255]; 165 * 162]);
    let request = pattern_request(&pattern(&tile, side, side), json!({}));
    assert!(request.to_string().len() > 64 * 1024);
    let out = typed_fill(doc, &request);
    let samples = layer16(out);
    for (x, y) in [(0, 0), (159, 0), (160, 1), (164, 161), (7, 160)] {
        let want = texel(&tile, side, side, x, y).map(|c| u16::from(c) * 257);
        assert_eq!(samples[(y * 165 + x) as usize], want, "({x}, {y})");
    }
    free(out);
    free(doc);
}

#[test]
fn float_fill_reads_code_over_255_as_linear_light() {
    let profile = IccProfile::srgb();
    let records = vec![vec![[0.3f32, 0.3, 0.3, 1.0]; 20]];
    let doc = Box::into_raw(Box::new(float_doc(5, 4, &profile, &records)));
    let out = typed_fill(doc, &pattern_request(&pattern(&OPAQUE, TW, TH), json!({})));
    let samples = unsafe { &*out }
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap();
    for (i, p) in samples.iter().enumerate() {
        let (x, y) = ((i % 5) as i64, (i / 5) as i64);
        let t = texel(&OPAQUE, TW, TH, x, y);
        for c in 0..4 {
            let want = f32::from(t[c]) / 255.0;
            assert!(
                (p[c] - want).abs() <= 1e-6,
                "({x}, {y}) {c}: {} vs {want}",
                p[c]
            );
        }
    }
    free(out);
    free(doc);
}

#[test]
fn gray_fill_takes_a_neutral_texel_as_its_gray_code() {
    let doc = gray_doc(7, 4, &[50; 28]);
    let tile: Vec<[u8; 4]> = [10u8, 90, 200, 255, 0, 128]
        .iter()
        .map(|&v| [v, v, v, 255])
        .collect();
    let out = typed_fill(doc, &pattern_request(&pattern(&tile, TW, TH), json!({})));
    for (i, p) in layer16(out).iter().enumerate() {
        let (x, y) = ((i % 7) as i64, (i / 7) as i64);
        let v = u16::from(texel(&tile, TW, TH, x, y)[0]) * 257;
        assert_eq!(*p, [v, v, v, 65535], "({x}, {y})");
    }
    free(out);
    free(doc);
}

/// For a mode whose texel reading is an existing conversion: the w × h
/// fill read through `read` equals, pixel by pixel, the 1 × 1 fill of the
/// texel the rule names (module doc), and the tile is not uniform there.
fn assert_texel_rule<T: PartialEq + std::fmt::Debug + Clone>(
    doc: *const PrecisionDocument,
    extra: Value,
    read: impl Fn(*const PrecisionDocument) -> Vec<T>,
    what: &str,
) {
    let (w, _) = unsafe { &*doc }.dimensions();
    let filled = typed_fill(
        doc,
        &pattern_request(&pattern(&OPAQUE, TW, TH), extra.clone()),
    );
    let got = read(filled);
    let singles: Vec<Vec<T>> = OPAQUE
        .iter()
        .map(|t| {
            let request = pattern_request(&pattern(&[*t], 1, 1), extra.clone());
            match paint(doc, &request, &full(doc)).unwrap_or_else(|e| panic!("{what}: {e}")) {
                Some(one) => {
                    let values = read(one);
                    free(one);
                    values
                }
                None => read(doc),
            }
        })
        .collect();
    for (i, value) in got.iter().enumerate() {
        let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
        let k =
            (y.rem_euclid(i64::from(TH)) * i64::from(TW) + x.rem_euclid(i64::from(TW))) as usize;
        assert_eq!(*value, singles[k][i], "{what} ({x}, {y})");
    }
    assert!(
        got.iter().any(|v| *v != got[0]),
        "{what}: the fill is not uniform"
    );
    free(filled);
}

#[test]
fn lab_cmyk_indexed_and_bitmap_fills_follow_the_texel_rule() {
    let rgb = rgb_doc(8, 5, &[[120, 60, 30, 255]; 40]);
    let options = CString::new(r#"{"depth":8}"#).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "to Lab: {}", take_err_string(err));
    assert_texel_rule(lab, json!({}), layer16, "Lab");
    free(lab);

    let cmyk = cmyk_layers(
        8,
        5,
        vec![RgbLayer {
            name: "Layer",
            w: 8,
            h: 5,
            offset: (0, 0),
            pixels: vec![[30000, 20000, 10000, 65535]; 40],
        }],
    );
    let cmyk_request = json!({"color": null, "color_space": null, "ink": [0, 0, 0, 0, 1]});
    let samples = |doc: *const PrecisionDocument| unsafe { &*doc }.cmyk_layer_samples(0).unwrap();
    assert_texel_rule(cmyk, cmyk_request, samples, "CMYK");
    free(cmyk);

    let mut scene: Vec<[u8; 4]> = OPAQUE.to_vec();
    scene.extend(vec![[255, 255, 255, 255]; 34]);
    let rgb_scene = rgb_doc(8, 5, &scene);
    let indexed = to_indexed(rgb_scene, &json!({"palette": "exact", "dither": "none"})).unwrap();
    free(rgb_scene);
    assert_texel_rule(indexed, json!({}), indices, "Indexed");
    free(indexed);

    let gray = gray_doc(8, 5, &[100; 40]);
    let bitmap = to_bitmap(gray, &json!({"method": "threshold"})).unwrap();
    free(gray);
    assert_texel_rule(bitmap, json!({}), bits, "Bitmap");
    free(bitmap);
    free(rgb);
}

#[test]
fn typed_pattern_refusals_name_what_is_wrong() {
    let doc = rgb_doc(4, 4, &[[255, 255, 255, 255]; 16]);
    let malformed = pattern_request(
        &json!({"width": 3, "height": 2, "rgba": base64(&[1, 2, 3])}),
        json!({}),
    );
    let refusal = paint(doc, &malformed, &full(doc)).unwrap_err();
    assert!(refusal.contains("pattern.rgba must be base64"), "{refusal}");
    let named = pattern_request(&json!({"name": "Bricks"}), json!({}));
    let refusal = paint(doc, &named, &full(doc)).unwrap_err();
    assert!(
        refusal.contains("pattern must hold either preset or rgba"),
        "{refusal}"
    );
    let unknown = pattern_request(&json!({"preset": "bricks"}), json!({}));
    assert!(paint(doc, &unknown, &full(doc)).is_err());
    free(doc);

    let profile = IccProfile::srgb();
    let float = Box::into_raw(Box::new(float_doc(4, 4, &profile, &[vec![[0.5; 4]; 16]])));
    let plane = pattern_request(&pattern(&OPAQUE, TW, TH), json!({"plane": "red"}));
    let refusal = paint(float, &plane, &full(float)).unwrap_err();
    assert!(refusal.contains("not a pattern"), "{refusal}");
    free(float);
}

fn linear_image(w: u32, h: u32, pixels: Vec<[f32; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba32_extended(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn tile_from_linear(image: &PrecisionImage) -> Result<(u32, u32, Vec<u8>), String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_pattern_tile_from_linear(image, &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let result = (img_dims(out).0, img_dims(out).1, img_pixels(out));
    unsafe { dr_image_free(out) };
    Ok(result)
}

#[test]
fn a_linear_tile_is_each_component_clamped_and_rounded() {
    let values = [-0.5f32, 0.0, 0.2, 0.5, 1.0, 1.7, 0.0019, 0.998];
    let pixels = vec![
        [values[0], values[1], values[2], values[3]],
        [values[4], values[5], values[6], values[7]],
    ];
    let (w, h, bytes) = tile_from_linear(&linear_image(2, 1, pixels)).unwrap();
    assert_eq!((w, h), (2, 1));
    let want: Vec<u8> = values
        .iter()
        .map(|v| (f64::from(*v).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    assert_eq!(bytes, want);

    let wide = linear_image(513, 1, vec![[0.5; 4]; 513]);
    assert!(tile_from_linear(&wide).unwrap_err().contains("at most 512"));
    let u16_image =
        PrecisionImage::from_rgba16(1, 1, vec![[1, 2, 3, 4]], IccProfile::srgb(), None).unwrap();
    assert!(tile_from_linear(&u16_image)
        .unwrap_err()
        .contains("32-bit float"));
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_pattern_tile_from_linear(ptr::null(), &mut err) }.is_null());
    assert!(!err.is_null());
    unsafe { dr_string_free(err) };
}

#[test]
fn a_float_tile_defined_on_the_code_grid_fills_back_exactly() {
    // Values on the k / 255 grid, as a float composite of 8-bit-made pixels.
    let source: Vec<[f32; 4]> = OPAQUE
        .iter()
        .map(|t| t.map(|c| f32::from(c) / 255.0))
        .collect();
    let (w, h, bytes) = tile_from_linear(&linear_image(TW, TH, source.clone())).unwrap();
    let tile: Vec<[u8; 4]> = bytes
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    let profile = IccProfile::srgb();
    let doc = Box::into_raw(Box::new(float_doc(6, 4, &profile, &[vec![[0.0; 4]; 24]])));
    let out = typed_fill(doc, &pattern_request(&pattern(&tile, w, h), json!({})));
    let samples = unsafe { &*out }
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap();
    for (i, p) in samples.iter().enumerate() {
        let (x, y) = (i as i64 % 6, i as i64 / 6);
        let k = (y.rem_euclid(2) * 3 + x.rem_euclid(3)) as usize;
        for c in 0..4 {
            assert!((p[c] - source[k][c]).abs() <= 1e-6, "({x}, {y}) {c}");
        }
    }
    free(out);
    free(doc);
}
