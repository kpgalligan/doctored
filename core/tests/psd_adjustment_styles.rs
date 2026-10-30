//! A layer style on an adjustment layer through PSD at 8 and
//! 16 bits. A style Photoshop can hold is WRITTEN on the adjustment record
//! beside its adjustment block — its effects (`lfx2`), Fill (`iOpa`) and
//! Blend If ranges — with any Pattern Overlay's pattern in the file's tail,
//! and the importer carries it back as the adjustment layer's style
//! (`psd.rs`, `psd_import16`). A style it cannot hold STAMPS the record
//! (`psd_export_preflight::stamps`): the layer is written as the picture up
//! to and including it, the loss names it, and the preflight charges the
//! stamp's scratch. Oracles: the record's own blocks as the file holds them,
//! and the reopened document's projection against the one exported (±1 code
//! per channel, declared before measuring).

use doctored_core::doc::DrDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{adjust_meta, ffi_style, styled};
use crate::psb_fixture::{self as fixture, LAYERS, PSD};

const W: u32 = 24;
const H: u32 = 16;

/// A chromatic opaque backdrop with a transparent hole and a 50 % band.
fn backdrop() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        let v = (12 + x * 10 + y) as u8;
        let a = match (x, y) {
            (2..=5, 5..=8) => 0,
            (6..=9, 5..=8) => 128,
            _ => 255,
        };
        Rgba([v, 255 - v, v / 2 + 60, a])
    })
}

fn scene() -> DrDocument {
    let mut doc = DrDocument::from_pixels(backdrop())
        .adding_image_layer(0, RgbaImage::new(W, H), "Adj")
        .unwrap();
    doc.layers[1].meta = Some(adjust_meta("invert", "{}"));
    doc
}

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
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A 12 × 8 opaque tile with distinct texels.
fn tile() -> String {
    let mut rgba = Vec::new();
    for y in 0..8u32 {
        for x in 0..12u32 {
            rgba.extend_from_slice(&[
                (20 * x + 10) as u8,
                (30 * y + 15) as u8,
                if (x + y) % 2 == 0 { 200 } else { 60 },
                255,
            ]);
        }
    }
    base64(&rgba)
}

/// Fill, Blend If, a Stroke and a Pattern Overlay whose pattern travels in
/// the file's tail.
fn held_style() -> String {
    json!({
        "fill_opacity": 0.75,
        "blend_if": {"channel": "gray", "this_layer": [0, 0, 200, 255],
                     "underlying": [0, 0, 255, 255]},
        "effects": [
            {"type": "stroke", "size": 2, "color": "#ff0000"},
            {"type": "pattern_overlay", "opacity": 0.5,
             "pattern": {"name": "S558", "width": 12, "height": 8, "rgba": tile()}},
        ],
    })
    .to_string()
}

/// A stroke under 1 px: Photoshop's dialog cannot hold it, so it bakes.
const BAKED: &str = r##"{"effects":[{"type":"stroke","size":0.5,"color":"#ff0000"}]}"##;

/// The ENABLED effects' types, in order (Photoshop also writes every
/// disabled effect it has defaults for).
fn effect_types(style: &str) -> Vec<String> {
    let value: Value = serde_json::from_str(style).unwrap();
    value["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["enabled"] != Value::Bool(false))
        .map(|e| e["type"].as_str().unwrap().to_string())
        .collect()
}

fn assert_close(a: &[u8], b: &[u8], what: &str) {
    assert_eq!(a.len(), b.len());
    for (i, (x, y)) in a.chunks(4).zip(b.chunks(4)).enumerate() {
        let channels = if x[3] == 0 && y[3] == 0 { 3..4 } else { 0..4 };
        for c in channels {
            assert!(x[c].abs_diff(y[c]) <= 1, "{what} at {i}: {x:?} vs {y:?}");
        }
    }
}

#[test]
fn an_adjustment_style_round_trips_through_an_8_bit_psd() {
    let dir = TempDir::new().unwrap();
    let doc = styled(&scene(), 1, &held_style());
    let report = fixture::export_report8(&doc, PSD, LAYERS);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert_eq!(report["bake_scratch_bytes"], json!(0), "nothing stamps");
    let path = dir.path().join("styled-adjustment8.psd");
    fixture::save8(&doc, &path, PSD, LAYERS).unwrap();
    let parsed = fixture::parse(&std::fs::read(&path).unwrap());
    let record = parsed.layers.iter().find(|l| l.name == "Adj").unwrap();
    assert!(record.block(b"nvrt").is_some(), "the adjustment record");
    assert!(record.block(b"lfx2").is_some(), "its effects");
    assert!(record.block(b"iOpa").is_some(), "its Fill");
    assert_eq!(record.bounds, [0; 4], "a pixel-less record");
    let reopened = fixture::open8(&path, LAYERS).unwrap();
    assert!(
        reopened.layers[1].meta.is_some(),
        "still an adjustment layer"
    );
    let style = ffi_style(&*reopened, 1).expect("the style came back");
    assert_eq!(effect_types(&style), ["stroke", "pattern_overlay"]);
    let value: Value = serde_json::from_str(&style).unwrap();
    // `iOpa` is one byte: 0.75 travels as 191 / 255.
    assert!((value["fill_opacity"].as_f64().unwrap() - 0.75).abs() <= 1.0 / 255.0);
    assert_eq!(value["blend_if"]["this_layer"], json!([0, 0, 200, 255]));
    // No loss names the adjustment's style any more.
    let losses = fixture::strings(&fixture::import_report(&path), "verified_losses");
    assert!(
        !losses.iter().any(|l| l.contains("cannot carry")),
        "{losses:?}"
    );
    assert_close(
        reopened.flattened().unwrap().as_raw(),
        doc.flattened().unwrap().as_raw(),
        "8-bit round trip",
    );
}

#[test]
fn an_adjustment_style_round_trips_through_a_16_bit_psd() {
    let dir = TempDir::new().unwrap();
    let doc = styled(&scene(), 1, &held_style())
        .promote_structure(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    assert!(doc.layer(1).unwrap().style().is_some());
    let report = fixture::export_report16(&doc, PSD, LAYERS);
    assert_eq!(report["bake_scratch_bytes"], json!(0), "{report}");
    let path = dir.path().join("styled-adjustment16.psd");
    fixture::save16(&doc, &path, PSD, LAYERS).unwrap();
    let parsed = fixture::parse(&std::fs::read(&path).unwrap());
    let record = parsed.layers.iter().find(|l| l.name == "Adj").unwrap();
    assert!(record.block(b"nvrt").is_some() && record.block(b"lfx2").is_some());
    let reopened = fixture::open16(&path, LAYERS).unwrap();
    let style = reopened
        .layer(1)
        .unwrap()
        .reported_style()
        .expect("the style came back");
    assert_eq!(effect_types(&style), ["stroke", "pattern_overlay"]);
    assert!(reopened.layer(1).unwrap().blend_if().is_some());
    let (got, want) = (fixture::rendered16(&reopened), fixture::rendered16(&doc));
    for (i, (a, b)) in got.iter().zip(&want).enumerate() {
        for c in 0..4 {
            assert!(
                a[c].abs_diff(b[c]) <= 257,
                "16-bit round trip at {i}: {a:?} vs {b:?}"
            );
        }
    }
}

#[test]
fn a_style_photoshop_cannot_hold_stamps_the_adjustment_record() {
    let dir = TempDir::new().unwrap();
    let doc = styled(&scene(), 1, BAKED);
    let picture = doc.flattened().unwrap();
    let report = fixture::export_report8(&doc, PSD, LAYERS);
    assert_eq!(
        report["bake_scratch_bytes"],
        json!(3 * 4 * W * H),
        "{report}"
    );
    let losses = fixture::strings(&report, "losses");
    assert!(
        losses.iter().any(|l| l.starts_with(
            "Adjustment layer “Adj” (invert) has a layer style Photoshop cannot hold on an adjustment record, so it is written as a pixel layer with the style rasterized."
        )),
        "{losses:?}"
    );
    let path = dir.path().join("stamped8.psd");
    fixture::save8(&doc, &path, PSD, LAYERS).unwrap();
    let parsed = fixture::parse(&std::fs::read(&path).unwrap());
    let record = parsed.layers.iter().find(|l| l.name == "Adj").unwrap();
    assert!(record.block(b"nvrt").is_none() && record.block(b"lfx2").is_none());
    assert_eq!(
        record.bounds,
        [0, 0, H as i32, W as i32],
        "the canvas stamp"
    );
    let stamp: Vec<[u16; 4]> = picture.pixels().map(|p| p.0.map(u16::from)).collect();
    assert_eq!(record.rgba(1).unwrap(), stamp);
    let reopened = fixture::open8(&path, LAYERS).unwrap();
    assert!(reopened.layers[1].meta.is_none(), "a stamp is pixels");
    // The 16-bit preflight forecasts the same stamp and charges its scratch.
    let typed = doc
        .promote_structure(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    let report = fixture::export_report16(&typed, PSD, LAYERS);
    assert_eq!(
        report["bake_scratch_bytes"],
        json!(3 * 4 * 2 * W * H),
        "{report}"
    );
    let path = dir.path().join("stamped16.psd");
    fixture::save16(&typed, &path, PSD, LAYERS).unwrap();
    let parsed = fixture::parse(&std::fs::read(&path).unwrap());
    let record = parsed.layers.iter().find(|l| l.name == "Adj").unwrap();
    assert!(record.block(b"nvrt").is_none());
    assert_eq!(record.bounds, [0, 0, H as i32, W as i32]);
}

// ------------------------------------------------- the Photoshop take --
//
// `fixtures/psd_photoshop/adjustment_styles` (its README): Photoshop's own
// PSDs and renders of the take's scene, 192 × 128 with the hole at x
// 24..71 and the 50 % band at x 72..119, y 48..79. The bound (declared
// before measuring): 2 codes per channel on the regions where the take
// confirmed the shipped rule.

const TW: u32 = 192;
const TH: u32 = 128;

fn take_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/adjustment_styles")
}

fn take_png(name: &str) -> RgbaImage {
    image::open(take_dir().join(format!("{name}.png")))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .to_rgba8()
}

/// Opaque, at least `grow` px clear of the hole and the band.
fn clear(x: u32, y: u32, grow: u32) -> bool {
    !(x + grow >= 24 && x < 120 + grow && y + grow >= 48 && y < 80 + grow)
}

fn within(got: [u8; 4], want: [u8; 4], what: &str) {
    for c in 0..4 {
        assert!(got[c].abs_diff(want[c]) <= 2, "{what}: {got:?} vs {want:?}");
    }
}

#[test]
fn photoshops_take_psds_import_with_their_style() {
    let doc = fixture::open8(&take_dir().join("a3_rgb8.psd"), LAYERS).unwrap();
    let adjustment = doc
        .layers
        .iter()
        .position(|l| l.meta.is_some())
        .expect("the adjustment");
    let style = ffi_style(&*doc, adjustment).expect("its style came back");
    assert_eq!(effect_types(&style), ["stroke"], "{style}");
    let value: Value = serde_json::from_str(&style).unwrap();
    assert!(
        (value["fill_opacity"].as_f64().unwrap() - 0.5).abs() <= 1.0 / 255.0,
        "{style}"
    );
    // FILL-SCALES, confirmed by the take: the interior clear of the hole and
    // band (where the stroke is not) is Photoshop's own render.
    let flat = doc.flattened().unwrap();
    let photoshop = take_png("a3_rgb8");
    for y in 0..TH {
        for x in 0..TW {
            if clear(x, y, 8) {
                within(
                    flat.get_pixel(x, y).0,
                    photoshop.get_pixel(x, y).0,
                    &format!("a3 ({x}, {y})"),
                );
            }
        }
    }
    let shadowed = fixture::open8(&take_dir().join("a6_rgb8.psd"), LAYERS).unwrap();
    let adjustment = shadowed
        .layers
        .iter()
        .position(|l| l.meta.is_some())
        .unwrap();
    let style = ffi_style(&*shadowed, adjustment).expect("its style came back");
    assert_eq!(effect_types(&style), ["drop_shadow"], "{style}");
    // POST on Photoshop's own file: the imported adjustment inverts its own
    // shadow, so the render is Photoshop's on every pixel.
    let flat = shadowed.flattened().unwrap();
    let photoshop = take_png("a6_rgb8");
    for y in 0..TH {
        for x in 0..TW {
            within(
                flat.get_pixel(x, y).0,
                photoshop.get_pixel(x, y).0,
                &format!("a6 ({x}, {y})"),
            );
        }
    }
}

#[test]
fn rasterize_matches_photoshop_on_the_take_s_blend_if_rows() {
    // T-ADJUSTED (This Layer reads the adjusted colour) and the Underlying
    // row, confirmed by the take at 8 and 16 bits, rebuilt here over
    // Photoshop's own `base` render.
    let gray =
        |p: [u8; 4]| 0.30 * f64::from(p[0]) + 0.59 * f64::from(p[1]) + 0.11 * f64::from(p[2]);
    for (row, style) in [
        (
            "a4t",
            r#"{"blend_if":{"channel":"gray","this_layer":[0,0,128,128],"underlying":[0,0,255,255]}}"#,
        ),
        (
            "a4u",
            r#"{"blend_if":{"channel":"gray","this_layer":[0,0,255,255],"underlying":[128,128,255,255]}}"#,
        ),
    ] {
        for depth in [8, 16] {
            let base = take_png(&format!("base_rgb{depth}"));
            let photoshop = take_png(&format!("{row}_rgb{depth}"));
            let mut doc = DrDocument::from_pixels(base.clone())
                .adding_image_layer(0, RgbaImage::new(TW, TH), "Adj")
                .unwrap();
            doc.layers[1].meta = Some(adjust_meta("invert", "{}"));
            let doc = styled(&doc, 1, style);
            let rendered: Vec<[u8; 4]> = if depth == 8 {
                doc.flattened().unwrap().pixels().map(|p| p.0).collect()
            } else {
                let typed = doc
                    .promote_structure(REFERENCE_WORKING_BYTES, 0, None)
                    .unwrap();
                fixture::rendered16(&typed)
                    .iter()
                    .map(|p| p.map(|v| ((f64::from(v) / 257.0).round()) as u8))
                    .collect()
            };
            for y in 0..TH {
                for x in 0..TW {
                    let b = base.get_pixel(x, y).0;
                    if !clear(x, y, 2) || (gray(b) - 128.0).abs() <= 4.0 {
                        continue;
                    }
                    within(
                        rendered[(y * TW + x) as usize],
                        photoshop.get_pixel(x, y).0,
                        &format!("{row} RGB{depth} ({x}, {y})"),
                    );
                }
            }
        }
    }
}

#[test]
fn rasterize_matches_photoshop_on_the_take_s_shape_and_order_rows() {
    // S-MASK (a1: no stroke rings the hole; the band keeps its alpha), POST
    // (a2 and a6: the adjustment inverts its own Drop Shadow) and C-SHAPE on
    // that shape (a7), each rebuilt over Photoshop's own `base` render and
    // compared on EVERY pixel with the 2-code bound.
    const STROKE4: &str =
        r##"{"effects":[{"type":"stroke","size":4,"position":"outside","color":"#ff0000"}]}"##;
    const SHADOW8: &str = r##"{"effects":[{"type":"drop_shadow","opacity":1,"color":"#000000","distance":8,"size":0,"spread":0,"angle":180,"use_global_light":false,"layer_knocks_out":false}]}"##;
    let mask = || {
        std::sync::Arc::new(image::GrayImage::from_fn(TW, TH, |x, y| {
            image::Luma([if (48..144).contains(&x) && (32..96).contains(&y) {
                255
            } else {
                0
            }])
        }))
    };
    for (row, style, masked, member) in [
        ("a1", STROKE4, false, false),
        ("a2", SHADOW8, true, false),
        ("a6", SHADOW8, false, false),
        ("a7", SHADOW8, true, true),
    ] {
        for depth in [8, 16] {
            let base = take_png(&format!("base_rgb{depth}"));
            let photoshop = take_png(&format!("{row}_rgb{depth}"));
            let mut doc = DrDocument::from_pixels(base)
                .adding_image_layer(0, RgbaImage::new(TW, TH), "Adj")
                .unwrap();
            doc.layers[1].meta = Some(adjust_meta("invert", "{}"));
            if masked {
                doc.layers[1].mask = Some(mask());
                doc.layers[1].mask_enabled = true;
            }
            let mut doc = styled(&doc, 1, style);
            if member {
                doc = doc
                    .adding_image_layer(
                        1,
                        RgbaImage::from_fn(TW, TH, |x, y| {
                            if (96..160).contains(&x) && (24..88).contains(&y) {
                                Rgba([255, 0, 0, 255])
                            } else {
                                Rgba([0, 0, 0, 0])
                            }
                        }),
                        "Member",
                    )
                    .unwrap();
                doc.layers[2].clipped = true;
            }
            let rendered: Vec<[u8; 4]> = if depth == 8 {
                doc.flattened().unwrap().pixels().map(|p| p.0).collect()
            } else {
                let typed = doc
                    .promote_structure(REFERENCE_WORKING_BYTES, 0, None)
                    .unwrap();
                fixture::rendered16(&typed)
                    .iter()
                    .map(|p| p.map(|v| ((f64::from(v) / 257.0).round()) as u8))
                    .collect()
            };
            for y in 0..TH {
                for x in 0..TW {
                    let got = rendered[(y * TW + x) as usize];
                    let want = photoshop.get_pixel(x, y).0;
                    let what = format!("{row} RGB{depth} ({x}, {y})");
                    if got[3] == 0 && want[3] == 0 {
                        continue;
                    }
                    within(got, want, &what);
                }
            }
        }
    }
}
