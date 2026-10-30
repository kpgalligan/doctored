//! #578: the BYTE Opacity and Fill below 32 bits (`opacity_byte`). A
//! layer's Opacity and its style's Fill composite at `q = code / 255`, `code =
//! floor(255 v + 0.5 + 1e-3)` (`scripts/photoshop-probe/effects_578_take.py`),
//! at 8 bits, at 16 bits and in CMYK; 32-bit float composites the exact `v`.
//!
//! Every oracle is written here in f64 from source-over on an opaque white
//! backdrop: `out = W·(1 − a) + S·a` with `a = q` (Opacity or Fill) or, for
//! both, the two bytes' product rounded to a byte, `round(b·b / 255) / 255`
//! (`opacity_byte::joint`: fitted on effects-578 take1's `both_50`, confirmed
//! by the single-component rows; the product's own rounding is not
//! independently confirmed), and `q` from the integer rule above, never from
//! the core. At 8 bits
//! the colours are chosen so every oracle lies at least 0.02 code from a tie,
//! which is asserted as a precondition, and the oracle must differ from the
//! exact rule's somewhere, so the tests cannot pass on today's arithmetic.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_doc::{dr_doc_flattened, dr_doc_free};
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_doc_save_psd, dr_psd_import_report};
use doctored_core::ffi_regional::dr_doc_render_region;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_structure_fixture::{
    cmyk_layers, free, then, try_edit, with_fill, with_opacity, RgbLayer,
};
use crate::common::{cpath, img_dims, img_pixels, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// The values every test runs at: whole percents and Photoshop's own byte.
const VALUES: [f32; 6] = [0.1, 0.3, 0.5, 0.7, 0.9, 128.0 / 255.0];

/// Channel codes whose every oracle at `VALUES` (single and both) lies at
/// least 0.02 code from a tie (asserted by `away_from_ties`).
const PALETTE: [u8; 6] = [0, 25, 64, 130, 200, 240];

#[derive(Clone, Copy, Debug)]
enum Kind {
    Opacity,
    Fill,
    Both,
}

const KINDS: [Kind; 3] = [Kind::Opacity, Kind::Fill, Kind::Both];

// ------------------------------------------------------------ the oracle --

/// The byte, written from the integer rule on the f32 the layer
/// stores widened to f64.
fn byte(v: f32) -> f64 {
    (f64::from(v.clamp(0.0, 1.0)) * 255.0 + 0.5 + 1e-3).floor()
}

fn q(v: f32) -> f64 {
    byte(v) / 255.0
}

/// The share the layer lands with under `kind` at `v`: the byte rule's when
/// `byte_rule` (both: the two bytes' product rounded to a byte, which is
/// never a tie because 255 is odd), else the exact value's.
fn share(kind: Kind, v: f32, byte_rule: bool) -> f64 {
    let exact = f64::from(v);
    match (kind, byte_rule) {
        (Kind::Opacity | Kind::Fill, true) => q(v),
        (Kind::Opacity | Kind::Fill, false) => exact,
        (Kind::Both, true) => (byte(v) * byte(v) / 255.0).round() / 255.0,
        (Kind::Both, false) => exact * exact,
    }
}

/// `S` over opaque white at `a`, in codes at `top`.
fn over_white(s: f64, a: f64, top: f64) -> f64 {
    top * (1.0 - a) + s * a
}

fn source_pixel(i: usize) -> [u8; 3] {
    [PALETTE[i], PALETTE[(i + 1) % 6], PALETTE[(i + 2) % 6]]
}

fn away_from_ties(x: f64) -> bool {
    ((x - x.floor()) - 0.5).abs() >= 0.02
}

// ----------------------------------------------------------- the builders --

/// A Fill-only style holding exactly `fill` (the JSON reader keeps three
/// decimals, so the value is set on the parsed style).
fn style(fill: f32) -> Arc<LayerStyle> {
    let mut style = LayerStyle::from_json(r#"{"fill_opacity":0.5}"#).unwrap();
    style.fill_opacity = fill;
    Arc::new(style)
}

fn source8() -> RgbaImage {
    RgbaImage::from_fn(6, 1, |x, _| {
        let c = source_pixel(x as usize);
        Rgba([c[0], c[1], c[2], 255])
    })
}

fn white8(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]))
}

/// An opaque `source` over opaque white, the layer at `kind` and `v`.
fn byte_doc_of(source: RgbaImage, kind: Kind, v: f32) -> DrDocument {
    let (w, h) = source.dimensions();
    let mut doc = DrDocument::from_pixels(white8(w, h))
        .adding_image_layer(0, source, "Layer")
        .unwrap();
    if matches!(kind, Kind::Opacity | Kind::Both) {
        doc = doc.with_layer_opacity(1, v).unwrap();
    }
    if matches!(kind, Kind::Fill | Kind::Both) {
        doc.layers[1].style = Some(style(v));
    }
    doc
}

fn image16(img: &RgbaImage) -> PrecisionImage {
    let px = img
        .pixels()
        .map(|p| p.0.map(|v| u16::from(v) * 257))
        .collect();
    PrecisionImage::from_rgba16(img.width(), img.height(), px, IccProfile::srgb(), None).unwrap()
}

fn typed_doc(source: &RgbaImage, kind: Kind, v: f32) -> PrecisionDocument {
    let (w, h) = source.dimensions();
    let opacity = if matches!(kind, Kind::Opacity | Kind::Both) {
        v
    } else {
        1.0
    };
    let mut layer = Layer::raster("Layer", image16(source), (0, 0), opacity, true).unwrap();
    if matches!(kind, Kind::Fill | Kind::Both) {
        layer = layer.with_layer_style(Some(&*style(v)), (w, h)).unwrap();
    }
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("B", image16(&white8(w, h)), (0, 0), 1.0, true).unwrap(),
            layer,
        ],
    )
    .unwrap()
}

// --------------------------------------------------------------- 8 bits --

#[test]
fn the_8_bit_composite_reads_the_byte_rule_for_opacity_fill_and_both() {
    for kind in KINDS {
        let mut differs_somewhere = false;
        for v in VALUES {
            let flat = byte_doc_of(source8(), kind, v).flattened().unwrap();
            let a = share(kind, v, true);
            let exact = share(kind, v, false);
            for x in 0..6 {
                let c = source_pixel(x);
                let got = flat.get_pixel(x as u32, 0).0;
                for k in 0..3 {
                    let want = over_white(f64::from(c[k]), a, 255.0);
                    assert!(
                        away_from_ties(want),
                        "precondition: {kind:?} {v} code {} gives {want}, a tie",
                        c[k]
                    );
                    assert_eq!(
                        f64::from(got[k]),
                        want.round(),
                        "{kind:?} at {v}: pixel {x} channel {k} (oracle {want})"
                    );
                    differs_somewhere |=
                        want.round() != over_white(f64::from(c[k]), exact, 255.0).round();
                }
            }
        }
        assert!(
            differs_somewhere,
            "{kind:?}: the byte rule must move a pixel"
        );
    }
}

#[test]
fn half_opacity_and_a_30_percent_fill_land_on_the_byte_rules_codes() {
    // R = 48 at 50 %: 255 − 207·128/255 = 151.094 → 151; the exact rule sits
    // on the 151.5 tie.
    let red48 = RgbaImage::from_pixel(1, 1, Rgba([48, 255, 255, 255]));
    let want = over_white(48.0, q(0.5), 255.0);
    assert!(
        away_from_ties(want) && (want - 151.094).abs() < 1e-3,
        "{want}"
    );
    assert_eq!(over_white(48.0, 0.5, 255.0), 151.5);
    let got = byte_doc_of(red48, Kind::Opacity, 0.5).flattened().unwrap();
    assert_eq!(got.get_pixel(0, 0).0[0], 151);
    // R = 14 at Fill 30 %: 255 − 241·77/255 = 182.227 → 182; the exact rule
    // gives 182.7 → 183.
    let red14 = RgbaImage::from_pixel(1, 1, Rgba([14, 255, 255, 255]));
    let want = over_white(14.0, q(0.3), 255.0);
    assert!(
        away_from_ties(want) && (want - 182.227).abs() < 1e-3,
        "{want}"
    );
    assert_eq!(over_white(14.0, 0.3, 255.0).round(), 183.0);
    let got = byte_doc_of(red14, Kind::Fill, 0.3).flattened().unwrap();
    assert_eq!(got.get_pixel(0, 0).0[0], 182);
}

/// effects-578 take1's `both_50`: A3's `#40607c` at Opacity 50 % and Fill
/// 50 % lands at 64 / 255 = `round(128 · 128 / 255) / 255` over every
/// backdrop code (Photoshop 0 / 0 codes there), where the unrounded product
/// `(128 / 255)²` misses by a code. `(191 b + 64 s) / 255` is never a tie.
#[test]
fn half_opacity_and_half_fill_together_land_on_the_product_byte() {
    let overlay = [0x40u8, 0x60, 0x7c];
    let backdrop = RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, x as u8, x as u8, 255]));
    let source = RgbaImage::from_pixel(256, 1, Rgba([overlay[0], overlay[1], overlay[2], 255]));
    let mut doc = DrDocument::from_pixels(backdrop)
        .adding_image_layer(0, source, "Layer")
        .unwrap()
        .with_layer_opacity(1, 0.5)
        .unwrap();
    doc.layers[1].style = Some(style(0.5));
    let flat = doc.flattened().unwrap();
    let mut moved = 0;
    for x in 0..256u32 {
        let got = flat.get_pixel(x, 0).0;
        for k in 0..3 {
            let (b, s) = (f64::from(x), f64::from(overlay[k]));
            let want = (b * 191.0 + s * 64.0) / 255.0;
            assert_eq!(
                f64::from(got[k]),
                want.round(),
                "backdrop {x} channel {k} ({want})"
            );
            let a = (128.0f64 / 255.0).powi(2);
            moved += usize::from(want.round() != (b * (1.0 - a) + s * a).round());
        }
    }
    assert!(
        moved > 0,
        "the product byte must move a pixel against (128 / 255)²"
    );
}

#[test]
fn a_fill_edit_and_an_opacity_edit_of_the_same_value_are_byte_identical() {
    let source = RgbaImage::from_fn(16, 8, |x, y| {
        Rgba([
            (x * 16) as u8,
            (y * 31) as u8,
            ((x + y) * 9) as u8,
            (128 + x * 8) as u8,
        ])
    });
    for v in VALUES.iter().copied().chain([0.333, 0.0049, 0.998]) {
        let fill = byte_doc_of(source.clone(), Kind::Fill, v)
            .flattened()
            .unwrap();
        let opacity = byte_doc_of(source.clone(), Kind::Opacity, v)
            .flattened()
            .unwrap();
        assert_eq!(
            fill.as_raw(),
            opacity.as_raw(),
            "Fill {v} against opacity {v}"
        );
    }
}

// ------------------------------------------------------ 16 and 32 bits --

#[test]
fn the_16_bit_composite_reads_the_byte_rule_within_one_code() {
    for kind in KINDS {
        for v in VALUES {
            let render = typed_doc(&source8(), kind, v)
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba16()
                .unwrap();
            let a = share(kind, v, true);
            let exact = share(kind, v, false);
            for (x, got) in render.iter().enumerate() {
                let c = source_pixel(x);
                for k in 0..3 {
                    let s = f64::from(c[k]) * 257.0;
                    let want = over_white(s, a, 65535.0);
                    assert!(
                        (f64::from(got[k]) - want).abs() <= 1.0,
                        "16-bit {kind:?} at {v}: pixel {x} channel {k}: {} vs {want}",
                        got[k]
                    );
                    if v == 0.5 && c[k] == 0 {
                        // The exact rule is 128.5 codes away here (64.2 for
                        // both: 64 / 255 against 0.25).
                        let off = over_white(s, exact, 65535.0);
                        assert!((want - off).abs() > 60.0, "{kind:?}: {want} vs {off}");
                    }
                }
            }
        }
    }
}

#[test]
fn the_32_bit_float_composite_keeps_the_exact_value() {
    // Black over white, so the linear float values are the encoded ones.
    let black = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]));
    for kind in KINDS {
        for v in VALUES {
            let float = typed_doc(&black, kind, v)
                .promote_to_float(LIMIT, 0, None)
                .unwrap();
            assert!(float.is_float());
            let render = float.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap();
            let want = 1.0 - share(kind, v, false);
            for got in &render {
                for value in &got[..3] {
                    assert!(
                        (f64::from(*value) - want).abs() <= 1e-6,
                        "float {kind:?} at {v}: {value} vs {want}"
                    );
                }
            }
            let byte_rule = 1.0 - share(kind, v, true);
            if (byte_rule - want).abs() > 1e-4 {
                let got = f64::from(render[0][0]);
                assert!((got - byte_rule).abs() > 1e-4, "float {kind:?} at {v}");
            }
        }
    }
}

// ------------------------------------------------------------------ CMYK --

/// Four inks (percent): paper, K 50, a mix and cyan.
const INKS: [[f64; 4]; 4] = [
    [0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 50.0],
    [20.0, 40.0, 60.0, 10.0],
    [100.0, 0.0, 0.0, 0.0],
];

fn paint(doc: *mut PrecisionDocument, layer: usize, rows: bool) -> *mut PrecisionDocument {
    let mut doc = doc;
    for (i, ink) in INKS.iter().enumerate() {
        let stroke: Vec<u16> = (0..16u32)
            .map(|p| {
                let at = if rows { p / 4 } else { p % 4 };
                if at as usize == i {
                    65535
                } else {
                    0
                }
            })
            .collect();
        let request = format!(
            r#"{{"op":"paint_blend","layer":{layer},"ink":[{},{},{},{},1],"opacity":1}}"#,
            ink[0] / 100.0,
            ink[1] / 100.0,
            ink[2] / 100.0,
            ink[3] / 100.0
        );
        doc = then(doc, &request, &stroke);
    }
    doc
}

/// The 4 × 4 grid: backdrop inks by row, source inks by column, both opaque.
fn cmyk_grid() -> *mut PrecisionDocument {
    let doc = cmyk_layers(
        4,
        4,
        vec![RgbLayer {
            name: "Backdrop",
            w: 4,
            h: 4,
            offset: (0, 0),
            pixels: vec![[65535; 4]; 16],
        }],
    );
    let doc = paint(doc, 0, true);
    let doc = then(
        doc,
        r#"{"op":"add_layer","name":"Source","anchor":0,"position":"above"}"#,
        &[],
    );
    paint(doc, 1, false)
}

fn cmyk_sample(doc: *const PrecisionDocument, x: i32, y: i32) -> [u16; 5] {
    crate::cmyk_structure_fixture::composite(doc, x, y)
}

#[test]
fn the_8_bit_cmyk_composite_reads_the_byte_rule() {
    let grid = cmyk_grid();
    let (back, source) = unsafe {
        (
            (*grid).cmyk_layer_samples(0).unwrap(),
            (*grid).cmyk_layer_samples(1).unwrap(),
        )
    };
    for kind in KINDS {
        let mut differs_somewhere = false;
        for v in VALUES {
            // A rename gives a new handle and leaves the grid as it was.
            let mut d =
                try_edit(grid, r#"{"op":"rename_layer","layer":1,"name":"Src"}"#, &[]).unwrap();
            if matches!(kind, Kind::Opacity | Kind::Both) {
                d = with_opacity(d, 1, v);
            }
            if matches!(kind, Kind::Fill | Kind::Both) {
                d = with_fill(d, 1, f64::from(v));
            }
            let a = share(kind, v, true);
            let exact = share(kind, v, false);
            for y in 0..4 {
                for x in 0..4 {
                    let i = (y * 4 + x) as usize;
                    let got = cmyk_sample(d, x, y);
                    for plate in 0..4 {
                        // Source-over on L = 1 − ink: L_o = a·L_s + (1 − a)·L_b.
                        let lb = 1.0 - f64::from(back[i][plate]) / 65535.0;
                        let ls = 1.0 - f64::from(source[i][plate]) / 65535.0;
                        let want = (1.0 - (a * ls + (1.0 - a) * lb)) * 65535.0;
                        assert!(
                            (f64::from(got[plate]) - want).abs() <= 2.0,
                            "CMYK {kind:?} at {v}: ({x}, {y}) plate {plate}: {} vs {want}",
                            got[plate]
                        );
                        let off = (1.0 - (exact * ls + (1.0 - exact) * lb)) * 65535.0;
                        differs_somewhere |= (want - off).abs() > 2.0;
                    }
                }
            }
            free(d);
        }
        assert!(
            differs_somewhere,
            "CMYK {kind:?}: the byte rule must move a sample"
        );
    }
    free(grid);
}

// -------------------------------------------------------------- regional --

fn flattened(doc: &DrDocument) -> Vec<u8> {
    let h = Box::into_raw(Box::new(doc.clone()));
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_flattened(h, 0, ptr::null(), &mut err) };
    assert!(!img.is_null(), "flattened: {}", take_err_string(err));
    let out = img_pixels(img);
    unsafe {
        dr_image_free(img);
        dr_doc_free(h);
    }
    out
}

fn region(doc: &DrDocument, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u8> {
    let hd = Box::into_raw(Box::new(doc.clone()));
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, 1, u64::MAX, ptr::null(), &mut err) };
    unsafe { dr_doc_free(hd) };
    assert!(!img.is_null(), "region: {}", take_err_string(err));
    assert_eq!(img_dims(img), (w, h));
    let px = img_pixels(img);
    unsafe { dr_image_free(img) };
    px
}

#[test]
fn a_regional_render_at_half_opacity_and_half_fill_equals_the_full_render() {
    let source = RgbaImage::from_fn(17, 11, |x, y| {
        Rgba([(x * 15) as u8, (y * 23) as u8, 90, (100 + x * 9) as u8])
    });
    for kind in KINDS {
        let doc = byte_doc_of(source.clone(), kind, 0.5);
        let full = flattened(&doc);
        assert_eq!(
            region(&doc, (0, 0, 17, 11)),
            full,
            "{kind:?}: the whole canvas"
        );
        let window = region(&doc, (3, 2, 9, 5));
        for j in 0..5usize {
            for i in 0..9usize {
                let at = ((2 + j) * 17 + 3 + i) * 4;
                let w = (j * 9 + i) * 4;
                assert_eq!(
                    &window[w..w + 4],
                    &full[at..at + 4],
                    "{kind:?} at ({i}, {j})"
                );
            }
        }
    }
}

// ------------------------------------------------------------- PSD bytes --

fn reopened(doc: &DrDocument, dir: &TempDir, name: &str) -> Box<DrDocument> {
    let path = dir.path().join(name);
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_psd(doc, ptr::null(), c.as_ptr(), 0, 0, &mut err) },
        "save: {}",
        take_err_string(err)
    );
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(c.as_ptr(), &mut err) };
    assert!(!json.is_null(), "import report: {}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { doctored_core::ffi::dr_string_free(json) };
    let report: Value = serde_json::from_str(&text).unwrap();
    let f = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_open_psd(c.as_ptr(), 0, f.as_ptr(), &mut err) };
    assert!(!out.is_null(), "open: {}", take_err_string(err));
    unsafe { Box::from_raw(out) }
}

/// The PSD writer's opacity byte and `iOpa` are `code(v)`, read back through
/// the importer (whose value reads back as the byte it loaded): 179 at 70 %, and 179
/// at 178.4995 / 255, where a half-up rounding of the f32 product gives 178;
/// and the composite there equals the composite at 179 / 255.
#[test]
fn the_psd_writer_writes_the_byte_it_composites() {
    let dir = TempDir::new().unwrap();
    let tie: f32 = (178.4995f64 / 255.0) as f32;
    assert_eq!(
        ((tie.clamp(0.0, 1.0)) * 255.0).round(),
        178.0,
        "precondition"
    );
    assert_eq!(byte(tie), 179.0, "precondition");
    for v in [0.7f32, tie] {
        let doc = byte_doc_of(source8(), Kind::Both, v);
        let back = reopened(&doc, &dir, &format!("byte_{}.psd", v.to_bits()));
        let layer = &back.layers[1];
        assert_eq!(byte(layer.opacity), 179.0, "the opacity byte at {v}");
        let fill = layer.style.as_ref().expect("a Fill style").fill_opacity;
        assert_eq!(
            byte(fill),
            179.0,
            "the iOpa byte at {v} (read back as {fill})"
        );
    }
    let at = |v: f32| byte_doc_of(source8(), Kind::Both, v).flattened().unwrap();
    assert_ne!(tie, 179.0 / 255.0);
    assert_eq!(at(tie).as_raw(), at(179.0 / 255.0).as_raw());
}
