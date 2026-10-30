//! The scoped-analysis request's `auto` — Levels Auto Tone / Auto
//! Color measured over a backdrop, weighted where the adjustment can act.
//!
//! Oracles are independent of the implementation: the 8-bit parity oracle is
//! `dr_image_auto_levels` on the very layer the backdrop shows, the 16-bit
//! one `dr_precision_doc_selected_auto_levels` on that layer, the group and
//! clip ones the byte Auto of `dr_doc_flattened` over the stack cut at the
//! boundary (the whole-backdrop scope), and every weighted case the
//! documented Tone rule written out HERE (`weighted_tone`). Every fixture is
//! opaque, with a tonally distinct layer under the measured content.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_adjust::dr_image_auto_levels;
use doctored_core::ffi_doc::dr_doc_flattened;
use doctored_core::ffi_scoped_statistics::{
    dr_doc_scoped_analysis, dr_doc_scoped_analysis_plan, dr_precision_doc_scoped_analysis,
    dr_precision_doc_scoped_analysis_plan,
};
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_auto_levels;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::tiff_float::FloatTiff;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 24;
const H: u32 = 16;
const N: usize = (W * H) as usize;
/// The photo's rect inside the canvas, for the clip and group cases.
const PHOTO: (i32, i32, u32, u32) = (6, 4, 12, 8);

// ------------------------------------------------------------- fixtures --

/// A low-contrast, cast photo: each channel inside its own range (min and max
/// both present), plus a 1/6 band of pixels that stretch to a near neutral
/// (0.47, 0.5, 0.53), so Auto Color has midtones to snap.
fn photo(w: u32, h: u32) -> RgbaImage {
    let ranges = [(40u32, 180u32), (60, 200), (30, 150)];
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        if i == 0 {
            return Rgba([40, 60, 30, 255]);
        }
        if i == 1 {
            return Rgba([180, 200, 150, 255]);
        }
        if y % 6 == 3 {
            let t = [0.47, 0.5, 0.53];
            let c = |k: usize| {
                let (lo, hi) = ranges[k];
                (lo as f64 + t[k] * (hi - lo) as f64).round() as u8
            };
            return Rgba([c(0), c(1), c(2), 255]);
        }
        let v = (x * 37 + y * 101 + 13) % 997;
        let c = |k: usize, m: u32| {
            let (lo, hi) = ranges[k];
            (lo + (v * m) % (hi - lo + 1)) as u8
        };
        Rgba([c(0, 7), c(1, 11), c(2, 3), 255])
    })
}

fn fill(value: u8) -> RgbaImage {
    RgbaImage::from_pixel(W, H, Rgba([value, value, value, 255]))
}

fn clear() -> RgbaImage {
    RgbaImage::new(W, H)
}

fn levels_meta() -> String {
    json!({"type":"adjust","op":"levels","params":{}}).to_string()
}
fn typed_levels() -> String {
    json!({"type":"adjust","op":"levels","params":{},"evaluation":"precision_v1"}).to_string()
}
/// `[photo(0), adjustment(1)]` at 8 bits, the adjustment's meta given.
fn byte_two(meta: &str) -> DrDocument {
    let doc = DrDocument::from_pixels(photo(W, H));
    let mut doc = doc.adding_image_layer(0, clear(), "Levels").unwrap();
    doc.layers[1].meta = Some(meta.into());
    doc
}

fn promote(image: &RgbaImage) -> PrecisionImage {
    let pixels = image
        .pixels()
        .map(|p| p.0.map(|c| u16::from(c) * 257))
        .collect();
    PrecisionImage::from_rgba16(
        image.width(),
        image.height(),
        pixels,
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

fn raster(name: &str, image: &RgbaImage, offset: (i32, i32), visible: bool) -> Layer {
    Layer::raster(name, promote(image), offset, 1., visible).unwrap()
}

fn adjustment(description: &str) -> Layer {
    Layer::adjustment("Levels", description, 1., true, &IccProfile::srgb()).unwrap()
}

fn typed(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn typed_mask(coverage: &[u16]) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(W, H, coverage.to_vec(), None).unwrap(),
        MaskProperties::at((0, 0)),
        true,
    )
    .unwrap()
}

/// The u16 selection the mask cases share: 0, 128·257 and 255·257 bands.
fn selection() -> Vec<u16> {
    (0..N)
        .map(|i| match (i % W as usize) / 8 {
            0 => 0,
            1 => 128 * 257,
            _ => 65535,
        })
        .collect()
}

// ------------------------------------------------------------- requests --

fn request(below: usize, auto: Value) -> Value {
    json!({"operation":"histogram","source":"backdrop","below":below,"auto":auto})
}

unsafe fn answer(value: *mut c_char, error: *mut c_char) -> Result<Value, String> {
    if value.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        return Err(message);
    }
    let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(value) }.to_bytes()).unwrap();
    unsafe { dr_string_free(value) };
    Ok(parsed)
}

fn byte(doc: &DrDocument, request: &Value, coverage: &[u16]) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    unsafe {
        let out = dr_doc_scoped_analysis(
            doc,
            text.as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        );
        answer(out, error)
    }
}

fn wide(doc: &PrecisionDocument, request: &Value, coverage: &[u16]) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    unsafe {
        let out = dr_precision_doc_scoped_analysis(
            doc,
            text.as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        );
        answer(out, error)
    }
}

fn byte_plan(doc: &DrDocument, request: &Value) -> Result<u64, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let plan = unsafe {
        answer(
            dr_doc_scoped_analysis_plan(doc, text.as_ptr(), &mut error),
            error,
        )
    }?;
    Ok(plan["scratch_bytes"].as_u64().unwrap())
}

fn wide_plan(doc: &PrecisionDocument, request: &Value) -> Result<u64, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let plan = unsafe {
        answer(
            dr_precision_doc_scoped_analysis_plan(doc, text.as_ptr(), &mut error),
            error,
        )
    }?;
    Ok(plan["scratch_bytes"].as_u64().unwrap())
}

/// The answer's nine numbers as f32 bits, black then white then gamma.
fn bits(auto: &Value) -> [u32; 9] {
    let mut out = [0; 9];
    for (part, key) in ["black", "white", "gamma"].into_iter().enumerate() {
        for c in 0..3 {
            out[part * 3 + c] = (auto[key][c].as_f64().unwrap() as f32).to_bits();
        }
    }
    out
}

fn identity_bits() -> [u32; 9] {
    [0., 0., 0., 1., 1., 1., 1., 1., 1.].map(f32::to_bits)
}

// --------------------------------------------------------------- oracles --

struct Owned(*mut DrImage);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { dr_image_free(self.0) };
    }
}

/// `dr_image_auto_levels` (mode 0 Tone, 2 Color) at clip 0.001, identity
/// when it declines (it answers false for an identity result).
fn byte_auto(image: *const DrImage, mode: i32) -> [u32; 9] {
    let mut params = [0f32; 9];
    if unsafe { dr_image_auto_levels(image, ptr::null(), mode, 0.001, params.as_mut_ptr()) } {
        params.map(f32::to_bits)
    } else {
        identity_bits()
    }
}

fn image_auto(image: &RgbaImage, mode: i32) -> [u32; 9] {
    let handle = Owned(unsafe {
        dr_image_from_rgba(image.as_raw().as_ptr(), image.width(), image.height())
    });
    byte_auto(handle.0, mode)
}

fn flattened_auto(doc: &DrDocument, mode: i32) -> [u32; 9] {
    let mut error = ptr::null_mut();
    let handle = Owned(unsafe { dr_doc_flattened(doc, 0, std::ptr::null(), &mut error) });
    assert!(!handle.0.is_null());
    byte_auto(handle.0, mode)
}

fn wide_layer_auto(doc: &PrecisionDocument, op: &str) -> [u32; 9] {
    let text = CString::new(
        json!({"layer":0,"op":op,"params":{"clip":0.001},"analysis_scope":"whole_target"})
            .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let value = unsafe {
        answer(
            dr_precision_doc_selected_auto_levels(
                doc,
                text.as_ptr(),
                ptr::null(),
                0,
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            ),
            error,
        )
    }
    .unwrap();
    bits(&value)
}

/// The documented Tone rule, written out (`verify_levels_auto_photoshop.py`'s
/// takes 4 and 5): `total` counts every pixel with weight and alpha, `cut =
/// clip·total`, `unit` one whole pixel's weight. Per channel the black is the
/// lowest code whose running weight EXCEEDS `cut`. The white at clip 0 is the
/// highest occupied code; above 0 each channel walks its 256 bins (a 16-bit
/// code binned round(c/257)) from the top with `T = cut + 3·unit`, clipping a
/// bin while `T − run ≥ 0.7·b` (never the lowest occupied); a pixel above any
/// channel's stop bin leaves every channel's kept set, and each white is the
/// kept pixels' top bin when it holds three pixels' weight, else one below
/// the lowest removed bin above it. A channel whose black is not below its
/// white stays (0, 1); gamma is 1.
fn weighted_tone(pixels: &[[u16; 4]], weights: &[u64], maximum: u16, clip: f64) -> [u32; 9] {
    let unit = 65535.0;
    let counted: Vec<([usize; 3], f64)> = pixels
        .iter()
        .zip(weights)
        .filter(|(p, w)| p[3] > 0 && **w > 0)
        .map(|(p, w)| ([0, 1, 2].map(|c| usize::from(p[c])), *w as f64))
        .collect();
    let total: f64 = counted.iter().map(|(_, w)| w).sum();
    let cut = clip * total;
    let size = usize::from(maximum) + 1;
    let bin = |v: usize| {
        if maximum == 255 {
            v
        } else {
            (2 * v + 257) / 514
        }
    };
    let mut native = vec![vec![0f64; size]; 3];
    let mut eight = [[0f64; 256]; 3];
    for (p, w) in &counted {
        for c in 0..3 {
            native[c][p[c]] += w;
            eight[c][bin(p[c])] += w;
        }
    }
    let black: [Option<usize>; 3] = std::array::from_fn(|c| {
        let mut run = 0.0;
        (0..size).find(|&v| {
            run += native[c][v];
            run > cut
        })
    });
    let white: [Option<f32>; 3] = if cut == 0.0 {
        std::array::from_fn(|c| {
            (0..size)
                .rev()
                .find(|&v| native[c][v] > 0.0)
                .map(|v| v as f32 / f32::from(maximum))
        })
    } else {
        let t = cut + 3.0 * unit;
        let stops: [usize; 3] = std::array::from_fn(|c| {
            let occupied: Vec<usize> = (0..256).rev().filter(|&v| eight[c][v] > 0.0).collect();
            let mut run = 0.0;
            for (k, &v) in occupied.iter().enumerate() {
                if k + 1 < occupied.len() && t - run >= 0.7 * eight[c][v] {
                    run += eight[c][v];
                    continue;
                }
                return v;
            }
            unreachable!()
        });
        let (mut kept, mut removed) = ([[0f64; 256]; 3], [[0f64; 256]; 3]);
        for (p, w) in &counted {
            let b = p.map(bin);
            let target = if (0..3).any(|c| b[c] > stops[c]) {
                &mut removed
            } else {
                &mut kept
            };
            for c in 0..3 {
                target[c][b[c]] += w;
            }
        }
        std::array::from_fn(|c| {
            let top = (0..256).rev().find(|&v| kept[c][v] > 0.0)?;
            let last = (top + 1..256).find(|&v| removed[c][v] > 0.0);
            let code = match last {
                Some(l) if kept[c][top] < 3.0 * unit => l - 1,
                _ => top,
            };
            Some(code as f32 / 255.0)
        })
    };
    let mut out = identity_bits();
    for c in 0..3 {
        if let (Some(lo), Some(hi)) = (black[c], white[c]) {
            let lo = lo as f32 / f32::from(maximum);
            if lo < hi {
                out[c] = lo.to_bits();
                out[3 + c] = hi.to_bits();
            }
        }
    }
    out
}

fn byte_pixels(image: &RgbaImage) -> Vec<[u16; 4]> {
    image.pixels().map(|p| p.0.map(u16::from)).collect()
}

fn render16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn photo_canvas() -> RgbaImage {
    let mut canvas = clear();
    image::imageops::overlay(
        &mut canvas,
        &photo(PHOTO.2, PHOTO.3),
        i64::from(PHOTO.0),
        i64::from(PHOTO.1),
    );
    canvas
}

// ----------------------------------------------------------------- tests --

#[test]
fn byte_parity_equals_the_destructive_auto_on_the_layer_below() {
    let doc = byte_two(&levels_meta());
    let doc = doc.adding_image_layer(1, fill(90), "top").unwrap();
    for (mode, code) in [("tone", 0), ("color", 2)] {
        let value = byte(&doc, &request(1, json!({"mode":mode})), &[]).unwrap();
        let auto = &value["auto"];
        assert_eq!(auto["empty"], false);
        assert_eq!(auto["mode"], mode);
        assert_eq!(bits(auto), image_auto(&photo(W, H), code), "{mode}");
        assert_ne!(bits(auto), identity_bits());
    }
    let color = byte(&doc, &request(1, json!({"mode":"color"})), &[]).unwrap();
    let gamma = &color["auto"]["gamma"];
    assert!((0..3).any(|c| gamma[c].as_f64().unwrap() != 1.0), "{gamma}");
}

#[test]
fn wide_parity_equals_the_native_auto_on_the_layer_below() {
    let doc = typed(vec![
        raster("photo", &photo(W, H), (0, 0), true),
        adjustment(&typed_levels()),
        raster("top", &fill(90), (0, 0), true),
    ]);
    for (mode, op) in [("tone", "auto_tone"), ("color", "auto_color")] {
        let value = wide(&doc, &request(1, json!({"mode":mode})), &[]).unwrap();
        assert_eq!(value["auto"]["empty"], false);
        assert_eq!(bits(&value["auto"]), wide_layer_auto(&doc, op), "{mode}");
    }
}

#[test]
fn pass_through_group_measures_the_gated_backdrop_and_follows_its_opacity() {
    // [black(0), photo(1, d1), levels(2, d1), G(3, pass through)].
    let doc = DrDocument::from_pixels(fill(0));
    let doc = doc.adding_image_layer(0, photo(W, H), "photo").unwrap();
    let mut doc = doc.adding_image_layer(1, clear(), "Levels").unwrap();
    doc.layers[2].meta = Some(levels_meta());
    let (mut doc, group, _, _) = doc.group_layers(&[1, 2], "G").unwrap();
    assert_eq!(group, 3);
    assert_eq!(doc.layers[3].blend, BlendMode::PassThrough);
    let full = byte(&doc, &request(2, json!({"mode":"tone"})), &[]).unwrap();
    doc.layers[3].opacity = 0.5;
    for mode in [("tone", 0), ("color", 2)] {
        let value = byte(&doc, &request(2, json!({"mode":mode.0})), &[]).unwrap();
        let mut cut = doc.clone();
        cut.layers.remove(2);
        assert_eq!(
            bits(&value["auto"]),
            flattened_auto(&cut, mode.1),
            "{}",
            mode.0
        );
    }
    let half = byte(&doc, &request(2, json!({"mode":"tone"})), &[]).unwrap();
    assert_ne!(bits(&half["auto"]), bits(&full["auto"]));
}

#[test]
fn clipped_or_grouped_adjustment_measures_the_whole_backdrop() {
    // A Levels layer's Auto counts the whole visible backdrop below the
    // layer: a clip base and an isolated group do not narrow it (measured
    // against Photoshop 26.10.0, scripts/verify_levels_auto_photoshop.py). White fill under a smaller
    // photo, so a narrowed scope would show as a white point below 1.
    let byte_clip = || {
        let doc = DrDocument::from_pixels(fill(255));
        let doc = doc.adding_image_layer(0, photo_canvas(), "photo").unwrap();
        let mut doc = doc.adding_image_layer(1, clear(), "Levels").unwrap();
        doc.layers[2].meta = Some(levels_meta());
        doc.layers[2].clipped = true;
        doc
    };
    let byte_group = || {
        let (mut doc, group, _, _) = {
            let mut doc = byte_clip();
            doc.layers[2].clipped = false;
            doc.group_layers(&[1, 2], "G").unwrap()
        };
        doc.layers[group].blend = BlendMode::Normal;
        doc
    };
    for doc in [byte_clip(), byte_group()] {
        for (mode, code) in [("tone", 0), ("color", 2)] {
            let value = byte(&doc, &request(2, json!({"mode":mode})), &[]).unwrap();
            let mut cut = doc.clone();
            cut.layers.remove(2);
            assert_eq!(bits(&value["auto"]), flattened_auto(&cut, code), "{mode}");
            assert_eq!(value["auto"]["white"], json!([1.0, 1.0, 1.0]));
        }
    }

    let wide_clip = |visible: bool| {
        typed(vec![
            raster("white", &fill(255), (0, 0), true),
            raster("photo", &photo_canvas(), (0, 0), true),
            Layer::adjustment("Levels", &typed_levels(), 1., visible, &IccProfile::srgb())
                .unwrap()
                .with_clipped(true),
        ])
    };
    let wide_group = |visible: bool| {
        let children = vec![
            raster("photo", &photo_canvas(), (0, 0), true),
            Layer::adjustment("Levels", &typed_levels(), 1., visible, &IccProfile::srgb()).unwrap(),
        ];
        typed(vec![
            raster("white", &fill(255), (0, 0), true),
            Layer::isolated_normal("G", children, 1., true).unwrap(),
        ])
    };
    for build in [
        &wide_clip as &dyn Fn(bool) -> PrecisionDocument,
        &wide_group,
    ] {
        let value = wide(&build(true), &request(2, json!({"mode":"tone"})), &[]).unwrap();
        let backdrop = render16(&build(false));
        assert_eq!(
            bits(&value["auto"]),
            weighted_tone(&backdrop, &vec![65535; N], 65535, 0.001)
        );
        let color = wide(&build(true), &request(2, json!({"mode":"color"})), &[]).unwrap();
        assert_eq!(color["auto"]["white"], json!([1.0, 1.0, 1.0]));
    }
}

#[test]
fn create_with_a_selection_equals_edit_with_that_mask_at_both_depths() {
    let s = selection();
    let insert = json!({"anchor":0,"position":"above"});
    for mode in ["tone", "color"] {
        // 8 bits: create over [photo] with S, then the layer masked by S.
        let base = DrDocument::from_pixels(photo(W, H));
        let created = byte(&base, &request(1, json!({"mode":mode,"insert":insert})), &s).unwrap();
        let mut masked = byte_two(&levels_meta());
        let plane: Vec<u8> = s.iter().map(|v| (v / 257) as u8).collect();
        masked.layers[1].mask = Some(std::sync::Arc::new(
            image::GrayImage::from_raw(W, H, plane).unwrap(),
        ));
        masked.layers[1].mask_enabled = true;
        let edited = byte(&masked, &request(1, json!({"mode":mode})), &[]).unwrap();
        assert_eq!(
            bits(&created["auto"]),
            bits(&edited["auto"]),
            "8-bit {mode}"
        );
        let code = if mode == "tone" { 0 } else { 2 };
        assert_ne!(bits(&created["auto"]), image_auto(&photo(W, H), code));
        // The selection's histogram half is the caller's coverage; the
        // masked edit's is whole.
        assert_eq!(created["selection"], true);
        assert_eq!(edited["selection"], false);

        let base = typed(vec![raster("photo", &photo(W, H), (0, 0), true)]);
        let created = wide(&base, &request(1, json!({"mode":mode,"insert":insert})), &s).unwrap();
        let masked = typed(vec![
            raster("photo", &photo(W, H), (0, 0), true),
            adjustment(&typed_levels()).with_mask(Some(typed_mask(&s))),
        ]);
        let edited = wide(&masked, &request(1, json!({"mode":mode})), &[]).unwrap();
        assert_eq!(
            bits(&created["auto"]),
            bits(&edited["auto"]),
            "16-bit {mode}"
        );
    }
}

#[test]
fn an_all_revealed_or_disabled_mask_keeps_parity() {
    let mut revealed = byte_two(&levels_meta());
    revealed.layers[1].mask = Some(std::sync::Arc::new(image::GrayImage::from_pixel(
        W,
        H,
        image::Luma([255]),
    )));
    revealed.layers[1].mask_enabled = true;
    let mut disabled = byte_two(&levels_meta());
    disabled.layers[1].mask = Some(std::sync::Arc::new(image::GrayImage::new(W, H)));
    disabled.layers[1].mask_enabled = false;
    for (mode, code) in [("tone", 0), ("color", 2)] {
        let expected = image_auto(&photo(W, H), code);
        for doc in [&byte_two(&levels_meta()), &revealed, &disabled] {
            let value = byte(doc, &request(1, json!({"mode":mode})), &[]).unwrap();
            assert_eq!(bits(&value["auto"]), expected, "{mode}");
        }
    }
    let full = typed(vec![
        raster("photo", &photo(W, H), (0, 0), true),
        adjustment(&typed_levels()).with_mask(Some(typed_mask(&[65535; N]))),
    ]);
    let bare = typed(vec![
        raster("photo", &photo(W, H), (0, 0), true),
        adjustment(&typed_levels()),
    ]);
    for (mode, op) in [("tone", "auto_tone"), ("color", "auto_color")] {
        let a = wide(&full, &request(1, json!({"mode":mode})), &[]).unwrap();
        let b = wide(&bare, &request(1, json!({"mode":mode})), &[]).unwrap();
        assert_eq!(bits(&a["auto"]), bits(&b["auto"]));
        assert_eq!(bits(&a["auto"]), wide_layer_auto(&bare, op));
    }
}

#[test]
fn half_weight_coverage_follows_the_weighted_rule() {
    let coverage: Vec<u16> = (0..N)
        .map(|i| if i % 3 == 0 { 65535 } else { 32768 })
        .collect();
    let weights: Vec<u64> = coverage.iter().map(|v| u64::from(*v)).collect();
    let insert = json!({"mode":"tone","insert":{"anchor":0,"position":"above"}});
    let base = DrDocument::from_pixels(photo(W, H));
    let value = byte(&base, &request(1, insert.clone()), &coverage).unwrap();
    assert_eq!(
        bits(&value["auto"]),
        weighted_tone(
            &byte_pixels(&photo(W, H)),
            &weights,
            255,
            f64::from(0.001_f32)
        )
    );
    let base = typed(vec![raster("photo", &photo(W, H), (0, 0), true)]);
    let value = wide(&base, &request(1, insert), &coverage).unwrap();
    let pixels: Vec<[u16; 4]> = photo(W, H)
        .pixels()
        .map(|p| p.0.map(|c| u16::from(c) * 257))
        .collect();
    assert_eq!(
        bits(&value["auto"]),
        weighted_tone(&pixels, &weights, 65535, 0.001)
    );
}

#[test]
fn nothing_to_measure_answers_empty_identity() {
    let check = |value: &Value| {
        assert_eq!(value["auto"]["empty"], true, "{value}");
        assert_eq!(value["auto"]["identity"], true);
        assert_eq!(bits(&value["auto"]), identity_bits());
    };
    // Bottom-most adjustment.
    let mut bottom = DrDocument::from_pixels(clear());
    bottom.layers[0].meta = Some(levels_meta());
    let bottom = bottom.adding_image_layer(0, photo(W, H), "photo").unwrap();
    check(&byte(&bottom, &request(0, json!({"mode":"tone"})), &[]).unwrap());
    let wide_bottom = typed(vec![
        adjustment(&typed_levels()),
        raster("photo", &photo(W, H), (0, 0), true),
    ]);
    check(&wide(&wide_bottom, &request(0, json!({"mode":"color"})), &[]).unwrap());
    // Everything below hidden.
    let doc = DrDocument::from_pixels(fill(255));
    let mut doc = doc.adding_image_layer(0, photo_canvas(), "photo").unwrap();
    doc.layers[0].visible = false;
    doc.layers[1].visible = false;
    let mut doc = doc.adding_image_layer(1, clear(), "Levels").unwrap();
    doc.layers[2].meta = Some(levels_meta());
    check(&byte(&doc, &request(2, json!({"mode":"tone"})), &[]).unwrap());
    let wide_hidden = typed(vec![
        raster("white", &fill(255), (0, 0), false),
        raster("photo", &photo_canvas(), (0, 0), false),
        adjustment(&typed_levels()),
    ]);
    check(&wide(&wide_hidden, &request(2, json!({"mode":"tone"})), &[]).unwrap());
    // A fully hiding mask.
    let mut hidden = byte_two(&levels_meta());
    hidden.layers[1].mask = Some(std::sync::Arc::new(image::GrayImage::new(W, H)));
    hidden.layers[1].mask_enabled = true;
    check(&byte(&hidden, &request(1, json!({"mode":"tone"})), &[]).unwrap());
    let wide_hiding = typed(vec![
        raster("photo", &photo(W, H), (0, 0), true),
        adjustment(&typed_levels()).with_mask(Some(typed_mask(&[0; N]))),
    ]);
    check(&wide(&wide_hiding, &request(1, json!({"mode":"tone"})), &[]).unwrap());
}

fn float_document() -> PrecisionDocument {
    let tiff = FloatTiff {
        width: W,
        height: H,
        channels: 4,
        alpha: Some(false),
        samples: (0..N)
            .flat_map(|i| [i as f32 / 400.0, 0.5, 0.25, 1.0])
            .collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).unwrap();
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("auto.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(doc.is_float());
    doc
}

#[test]
fn refusals_name_the_problem() {
    let doc = byte_two(&levels_meta());
    let tone = json!({"mode":"tone"});
    let refused = |value: Value, needle: &str| {
        let message = byte(&doc, &value, &[]).unwrap_err();
        assert!(message.contains(needle), "{message} / {needle}");
        let message = byte_plan(&doc, &value).unwrap_err();
        assert!(message.contains(needle), "{message} / {needle}");
    };
    let shape = "auto requires a backdrop histogram at stride 1";
    refused(
        json!({"operation":"histogram","source":"composite","auto":tone}),
        shape,
    );
    refused(
        json!({"operation":"histogram","source":"layer","layer":0,"auto":tone}),
        shape,
    );
    refused(
        json!({"operation":"sample","source":"backdrop","below":1,"x":0,"y":0,"auto":tone}),
        shape,
    );
    refused(
        json!({"operation":"histogram","source":"backdrop","below":1,"stride":2,"auto":tone}),
        shape,
    );
    let clip = "auto clip must be in 0...0.1";
    refused(request(1, json!({"mode":"tone","clip":0.2})), clip);
    refused(request(1, json!({"mode":"tone","clip":-0.001})), clip);
    // JSON has no NaN: the spelling is refused by the parser.
    refused(
        request(1, json!({"mode":"tone","clip":"NaN"})),
        "invalid scoped analysis request",
    );
    refused(
        request(1, json!({"mode":"contrast"})),
        "invalid scoped analysis request",
    );
    refused(
        request(1, json!({"mode":"tone","target":1})),
        "unknown field `target`",
    );
    refused(
        request(
            1,
            json!({"mode":"tone","insert":{"anchor":0,"position":"above","x":1}}),
        ),
        "unknown field `x`",
    );
    refused(
        request(
            2,
            json!({"mode":"tone","insert":{"anchor":0,"position":"above"}}),
        ),
        "auto insert must land at the backdrop boundary",
    );
    refused(
        request(
            1,
            json!({"mode":"tone","insert":{"anchor":0,"position":"into_group"}}),
        ),
        "into_group requires a group layer",
    );
    refused(
        request(
            1,
            json!({"mode":"tone","insert":{"anchor":9,"position":"above"}}),
        ),
        "layer insert anchor is out of range",
    );
    refused(
        request(0, tone.clone()),
        "auto without insert needs an adjustment layer at the backdrop boundary",
    );
    refused(
        request(2, tone.clone()),
        "auto without insert needs an adjustment layer at the backdrop boundary",
    );

    let wide_doc = typed(vec![raster("photo", &photo(W, H), (0, 0), true)]);
    let message = wide(&wide_doc, &request(1, tone.clone()), &[]).unwrap_err();
    assert!(message.contains("auto without insert needs an adjustment layer"));

    let float = float_document();
    let float_sentence = "automatic tone correction (Auto Tone, Auto Contrast, Auto Color and the Levels dialog's Auto buttons) is unavailable in a 32-bit float document";
    let message = wide_plan(&float, &request(1, tone.clone())).unwrap_err();
    assert!(message.contains(float_sentence), "{message}");
    let message = wide(&float, &request(1, tone), &[]).unwrap_err();
    assert!(message.contains(float_sentence), "{message}");
}

#[test]
fn the_plan_adds_the_derivation_scratch_and_the_mask_crop() {
    let doc = byte_two(&levels_meta());
    let plain = json!({"operation":"histogram","source":"backdrop","below":1});
    let base = byte_plan(&doc, &plain).unwrap();
    let canvas = u64::from(W * H);
    let byte_scratch = 3 * 256 * 8 + canvas * 2;
    assert_eq!(
        byte_plan(&doc, &request(1, json!({"mode":"tone"}))).unwrap(),
        base + byte_scratch
    );
    let mut masked = doc.clone();
    masked.layers[1].mask = Some(std::sync::Arc::new(image::GrayImage::new(W, H)));
    masked.layers[1].mask_enabled = true;
    assert_eq!(
        byte_plan(&masked, &request(1, json!({"mode":"tone"}))).unwrap(),
        byte_plan(&masked, &plain).unwrap() + byte_scratch + canvas
    );
    let base_doc = DrDocument::from_pixels(photo(W, H));
    let create = request(
        1,
        json!({"mode":"color","insert":{"anchor":0,"position":"above"}}),
    );
    assert_eq!(
        byte_plan(&base_doc, &create).unwrap(),
        byte_plan(&base_doc, &plain).unwrap() + byte_scratch
    );

    let wide_doc = typed(vec![
        raster("photo", &photo(W, H), (0, 0), true),
        adjustment(&typed_levels()).with_mask(Some(typed_mask(&[0; N]))),
    ]);
    assert_eq!(
        wide_plan(&wide_doc, &request(1, json!({"mode":"tone"}))).unwrap(),
        wide_plan(&wide_doc, &plain).unwrap() + 3 * 65536 * 8 + canvas * 2
    );
}

#[test]
fn the_histogram_half_is_unchanged_by_auto() {
    let doc = byte_two(&levels_meta());
    let s = selection();
    let plain = json!({"operation":"histogram","source":"backdrop","below":1});
    let mut with = byte(&doc, &request(1, json!({"mode":"tone"})), &s).unwrap();
    assert!(with.as_object_mut().unwrap().remove("auto").is_some());
    assert_eq!(with, byte(&doc, &plain, &s).unwrap());
    let wide_doc = typed(vec![
        raster("photo", &photo(W, H), (0, 0), true),
        adjustment(&typed_levels()),
    ]);
    let mut with = wide(&wide_doc, &request(1, json!({"mode":"color"})), &s).unwrap();
    assert!(with.as_object_mut().unwrap().remove("auto").is_some());
    assert_eq!(with, wide(&wide_doc, &plain, &s).unwrap());
    // Without auto, no auto key at all.
    assert!(byte(&doc, &plain, &[]).unwrap().get("auto").is_none());
}
