//! The Stroke's Fill Type Pattern at 8 bits, black-box
//! through `dr_doc_set_layer_style` and the projection. The placement
//! oracle is take1's tile T in closed form, `T(x, y) = (20x + 10, 30y + 15,
//! 200 if x + y is even else 60)` read at `((X − Ox) mod 12, (Y − Oy) mod 8)`
//! with O the Pattern Overlay's origin (`stroke_fill_take.py`'s PLACE =
//! OVERLAY), checked beside Photoshop's own PT-O/I/C/U TIFFs
//! (`fixtures/psd_photoshop/stroke_fill`) on the take's judged pixels. A
//! scaled and angled Inside band is held to the shipped Pattern Overlay's
//! pixels; then transparent texels, the JSON (written only when carried,
//! refusals under `pattern_fill`), the anchor rules, Scale Effects, Delete
//! Cropped Pixels and the byte `.rz` word 95 both ways. Never a golden.

use std::path::PathBuf;

use doctored_core::doc::DrDocument;
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use crate::common;
use common::*;

const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (13, 9, 50, 40);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];
const SIZE: i32 = 8;

// ---------------------------------------------------------------- helpers --

/// Standard base64 (RFC 4648, padded), written here so the test does not
/// borrow the core's encoder.
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
fn texel(x: u32, y: u32) -> [u8; 4] {
    [
        (20 * x + 10) as u8,
        (30 * y + 15) as u8,
        if (x + y).is_multiple_of(2) { 200 } else { 60 },
        255,
    ]
}

fn tile_json(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Value {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&f(x, y));
        }
    }
    json!({"name":"T","width":w,"height":h,"rgba":base64(&rgba)})
}

fn tile_t() -> Value {
    tile_json(12, 8, texel)
}

/// The texel T puts at canvas pixel (x, y) with its origin at `o`.
fn want(o: (i32, i32), x: i32, y: i32) -> [u8; 4] {
    texel(
        (x - o.0).rem_euclid(12) as u32,
        (y - o.1).rem_euclid(8) as u32,
    )
}

/// One 8 px Pattern stroke at `position` with `fill` keys over tile T.
fn stroke(position: &str, fill: Value) -> Value {
    let mut pattern_fill = json!({"pattern": tile_t()});
    for (k, v) in fill.as_object().unwrap() {
        pattern_fill[k] = v.clone();
    }
    json!({"effects":[{"type":"stroke","size":SIZE,"position":position,
        "fill_type":"pattern","pattern_fill":pattern_fill}]})
}

fn rect_doc() -> DrDocument {
    rect_layer_doc(CANVAS, RECT, GREEN)
}

fn effect0(doc: &DrDocument, idx: usize) -> Value {
    let style: Value = serde_json::from_str(&ffi_style(doc, idx).expect("a style")).unwrap();
    style["effects"][0].clone()
}

/// The take's judged pixels (`stroke_fill_take.py`): full band coverage on
/// the straight runs more than 9 px from every corner of `rect`, k = 1 ..
/// reach − 1 px from the contour on each side the band covers.
fn judged(rect: (i32, i32, u32, u32), position: &str) -> Vec<(i32, i32)> {
    let (x0, y0) = (rect.0, rect.1);
    let (x1, y1) = (x0 + rect.2 as i32 - 1, y0 + rect.3 as i32 - 1);
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

fn photoshop_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/stroke_fill")
        .join(name)
}

fn largest(a: [u8; 4], b: [u8; 4]) -> i32 {
    (0..3)
        .map(|c| (i32::from(a[c]) - i32::from(b[c])).abs())
        .max()
        .unwrap()
}

/// Every judged pixel of `doc` (band around `rect` at `position`) is T at
/// canvas origin `o` within one code, and so is Photoshop's `tiff` there.
fn assert_band(
    doc: &DrDocument,
    rect: (i32, i32, u32, u32),
    position: &str,
    o: (i32, i32),
    tiff: Option<&str>,
) {
    let ours = doc.flattened().unwrap();
    let theirs = tiff.map(|name| {
        DrDocument::open(photoshop_fixture(name).to_str().unwrap())
            .unwrap()
            .flattened()
            .unwrap()
    });
    let pixels = judged(rect, position);
    assert!(pixels.len() > 300, "{position}: {}", pixels.len());
    for (x, y) in pixels {
        let w = want(o, x, y);
        let got = ours.get_pixel(x as u32, y as u32).0;
        assert!(
            largest(got, w) <= 1,
            "{position} ({x}, {y}): {got:?} against T {w:?}"
        );
        if let Some(theirs) = &theirs {
            let ps = theirs.get_pixel(x as u32, y as u32).0;
            assert!(
                largest(got, ps) <= 1,
                "{position} ({x}, {y}): {got:?} against Photoshop {ps:?}"
            );
        }
    }
}

// ------------------------------------------------------------- placement --

#[test]
fn photoshops_pattern_strokes_are_the_overlays_tile_in_the_band() {
    for (position, tiff) in [
        ("outside", "pt_o_rgb8.tif"),
        ("inside", "pt_i_rgb8.tif"),
        ("center", "pt_c_rgb8.tif"),
    ] {
        let doc = styled(&rect_doc(), 1, &stroke(position, json!({})).to_string());
        assert_eq!(effect0(&doc, 1)["pattern_fill"]["anchor"], json!([13, 9]));
        assert_band(&doc, RECT, position, (0, 0), Some(tiff));
    }
    // PT-U: unlinked with phase (5, 2), the layer moved (+7, +3) after.
    let json = stroke(
        "outside",
        json!({"link_with_layer": false, "phase": [5, 2]}),
    );
    let doc = styled(&rect_doc(), 1, &json.to_string());
    assert!(effect0(&doc, 1)["pattern_fill"].get("anchor").is_none());
    let moved = doc.with_layer_offset(1, RECT.0 + 7, RECT.1 + 3).unwrap();
    let rect = (RECT.0 + 7, RECT.1 + 3, RECT.2, RECT.3);
    assert_band(&moved, rect, "outside", (5, 2), Some("pt_u_rgb8.tif"));
}

#[test]
fn a_linked_tile_follows_a_move_and_an_unlinked_one_stays() {
    let rect = (RECT.0 + 7, RECT.1 + 3, RECT.2, RECT.3);
    for position in ["outside", "inside", "center"] {
        let linked = styled(
            &rect_doc(),
            1,
            &stroke(position, json!({"phase": [5, 2]})).to_string(),
        );
        assert_band(&linked, RECT, position, (5, 2), None);
        let moved = linked.with_layer_offset(1, rect.0, rect.1).unwrap();
        assert_band(&moved, rect, position, (12, 5), None);
        let json = stroke(position, json!({"link_with_layer": false, "phase": [5, 2]}));
        let unlinked = styled(&rect_doc(), 1, &json.to_string());
        let moved = unlinked.with_layer_offset(1, rect.0, rect.1).unwrap();
        assert_band(&moved, rect, position, (5, 2), None);
    }
}

#[test]
fn delete_cropped_pixels_rebases_the_strokes_anchor_and_keeps_the_picture() {
    // A 40 x 30 layer made at (-5, -3) whose off-canvas part is transparent:
    // the trim moves its offset and leaves its shape, so the band is the
    // same and only the anchor tells the tile where it was.
    let mut pixels = image::RgbaImage::new(40, 30);
    for y in 8..24 {
        for x in 14..32 {
            pixels.put_pixel(x, y, image::Rgba(GREEN));
        }
    }
    let doc = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, WHITE))
        .adding_image_layer(0, pixels, "Rect")
        .unwrap();
    let mut doc = doc.with_layer_offset(1, -5, -3).unwrap();
    doc.layers[1].reference = (0, 0);
    let doc = styled(
        &doc,
        1,
        &stroke("center", json!({"phase": [1, 1]})).to_string(),
    );
    let before = doc.flattened().unwrap();
    let trimmed = doc
        .delete_cropped_pixels()
        .expect("the layer reaches outside");
    assert_eq!(trimmed.layers[1].offset, (0, 0));
    assert_eq!(
        effect0(&trimmed, 1)["pattern_fill"]["anchor"],
        json!([0, 0])
    );
    assert_eq!(
        trimmed.flattened().unwrap(),
        before,
        "the picture is unchanged"
    );
}

#[test]
fn a_scaled_angled_inside_band_is_the_pattern_overlays_pixels() {
    let fill = json!({"pattern": tile_t(), "scale": 0.5, "angle": 30, "phase": [3, 1]});
    let mut overlay = fill.clone();
    overlay["type"] = json!("pattern_overlay");
    let overlay = styled(&rect_doc(), 1, &json!({"effects": [overlay]}).to_string());
    let band = json!({"effects":[{"type":"stroke","size":SIZE,"position":"inside",
        "fill_type":"pattern","pattern_fill":fill}]});
    let band = styled(&rect_doc(), 1, &band.to_string());
    let (a, b) = (overlay.flattened().unwrap(), band.flattened().unwrap());
    let mut checked = 0;
    for (x, y) in judged(RECT, "inside") {
        assert_eq!(
            a.get_pixel(x as u32, y as u32).0,
            b.get_pixel(x as u32, y as u32).0,
            "({x}, {y})"
        );
        checked += 1;
    }
    assert!(checked > 300);
    // Inside the band's inner edge the layer is untouched.
    let centre = b.get_pixel(38, 29).0;
    assert_eq!(centre, GREEN);
}

#[test]
fn a_transparent_texel_shows_the_backdrop_outside_and_the_layer_inside() {
    // Columns 0..6 opaque red, 6..12 transparent.
    let half = tile_json(12, 8, |x, _| {
        if x < 6 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    for (position, under) in [("outside", [255u8; 4]), ("inside", GREEN)] {
        let json = json!({"effects":[{"type":"stroke","size":SIZE,"position":position,
            "fill_type":"pattern","pattern_fill":{"pattern":half}}]});
        let doc = styled(&rect_doc(), 1, &json.to_string());
        let flat = doc.flattened().unwrap();
        for (x, y) in judged(RECT, position) {
            let got = flat.get_pixel(x as u32, y as u32).0;
            let w = if x.rem_euclid(12) < 6 {
                [255, 0, 0, 255]
            } else {
                under
            };
            assert_eq!(got, w, "{position} ({x}, {y})");
        }
    }
}

// ------------------------------------------------------------------ JSON --

#[test]
fn pattern_fill_is_written_only_when_carried_and_refused_by_its_path() {
    let doc = rect_doc();
    let color = styled(&doc, 1, "{\"effects\":[{\"type\":\"stroke\"}]}");
    assert!(
        effect0(&color, 1).get("pattern_fill").is_none(),
        "a Color stroke"
    );
    let gradient = styled(
        &doc,
        1,
        "{\"effects\":[{\"type\":\"stroke\",\"fill_type\":\"gradient\"}]}",
    );
    assert!(
        effect0(&gradient, 1).get("pattern_fill").is_none(),
        "a Gradient stroke"
    );
    // A Pattern stroke without a `pattern_fill` writes the default fill.
    let bare = styled(
        &doc,
        1,
        "{\"effects\":[{\"type\":\"stroke\",\"fill_type\":\"pattern\"}]}",
    );
    let e = effect0(&bare, 1);
    assert_eq!(e["fill_type"], "pattern");
    assert_eq!(
        e["pattern_fill"],
        json!({"pattern":{"preset":"checkerboard"},"angle":0.0,"scale":1.0,
            "link_with_layer":true,"phase":[0.0,0.0],"anchor":[13,9]})
    );
    // A Color stroke that keeps a non-default fill writes it.
    let kept = styled(
        &doc,
        1,
        "{\"effects\":[{\"type\":\"stroke\",\"pattern_fill\":{\"scale\":2}}]}",
    );
    assert_eq!(effect0(&kept, 1)["pattern_fill"]["scale"], 2.0);
    for (json, message) in [
        (
            "{\"effects\":[{\"type\":\"stroke\",\"pattern_fill\":3}]}",
            "effects[0].pattern_fill must be an object",
        ),
        (
            "{\"effects\":[{\"type\":\"stroke\",\"pattern_fill\":{\"scale\":\"x\"}}]}",
            "effects[0].pattern_fill.scale must be a number",
        ),
        (
            "{\"effects\":[{\"type\":\"stroke\",\"pattern_fill\":{\"phase\":[1]}}]}",
            "effects[0].pattern_fill.phase must be an array of 2 numbers",
        ),
        (
            "{\"effects\":[{\"type\":\"stroke\",\"pattern_fill\":{\"anchor\":[1.5,2]}}]}",
            "effects[0].pattern_fill.anchor must be two integers",
        ),
        (
            "{\"effects\":[{\"type\":\"stroke\",\"pattern_fill\":{\"link_with_layer\":1}}]}",
            "effects[0].pattern_fill.link_with_layer must be a boolean",
        ),
        (
            "{\"effects\":[{\"type\":\"stroke\",\"pattern_fill\":{\"pattern\":{\"preset\":\"nope\"}}}]}",
            "effects[0].pattern_fill.pattern.preset must be one of checkerboard, stripes, dots, grid",
        ),
    ] {
        let got = style_error(&doc as *const DrDocument, 1, json);
        assert_eq!(got, Some(format!("style: {message}")), "{json}");
    }
    // Lenient (the `.rz` reader) takes the default for an unknown preset.
    let lenient = LayerStyle::from_json_lenient(
        "{\"effects\":[{\"type\":\"stroke\",\"fill_type\":\"pattern\",\
         \"pattern_fill\":{\"pattern\":{\"preset\":\"nope\"}}}]}",
    )
    .unwrap();
    let back: Value = serde_json::from_str(&lenient.to_json()).unwrap();
    assert_eq!(
        back["effects"][0]["pattern_fill"]["pattern"],
        json!({"preset":"checkerboard"})
    );
}

#[test]
fn a_color_stroke_gains_no_anchor_and_a_pattern_one_drops_it_unlinked() {
    let doc = rect_doc();
    let kept = styled(
        &doc,
        1,
        "{\"effects\":[{\"type\":\"stroke\",\"pattern_fill\":{\"scale\":2}}]}",
    );
    assert!(effect0(&kept, 1)["pattern_fill"].get("anchor").is_none());
    let carried = styled(
        &doc,
        1,
        &stroke("outside", json!({"anchor": [1, 2]})).to_string(),
    );
    assert_eq!(
        effect0(&carried, 1)["pattern_fill"]["anchor"],
        json!([1, 2])
    );
    // Echoing the style back changes nothing.
    let echo = ffi_style(&carried, 1).unwrap();
    assert!(try_styled(&carried, 1, Some(&echo)).unwrap().is_none());
    let json = stroke(
        "outside",
        json!({"link_with_layer": false, "anchor": [1, 2]}),
    );
    let unlinked = styled(&doc, 1, &json.to_string());
    assert!(effect0(&unlinked, 1)["pattern_fill"]
        .get("anchor")
        .is_none());
}

#[test]
fn scale_effects_scales_a_carried_fill_and_leaves_a_default_one() {
    let style = parse(
        &stroke(
            "outside",
            json!({"scale": 0.5, "phase": [3, -2], "anchor": [10, 5]}),
        )
        .to_string(),
    );
    let scaled: Value = serde_json::from_str(&style.scaled(2.0).to_json()).unwrap();
    let fill = &scaled["effects"][0]["pattern_fill"];
    assert_eq!(fill["scale"], 1.0);
    assert_eq!(fill["phase"], json!([6.0, -4.0]));
    assert_eq!(fill["anchor"], json!([20, 10]));
    assert_eq!(scaled["effects"][0]["size"], 16.0);
    // A Color stroke scaled x2 keeps its default fill: no `pattern_fill`,
    // and its file claims no word 95.
    let color = parse("{\"effects\":[{\"type\":\"stroke\",\"size\":3}]}").scaled(2.0);
    let json: Value = serde_json::from_str(&color.to_json()).unwrap();
    assert!(json["effects"][0].get("pattern_fill").is_none());
    let doc = styled(&rect_doc(), 1, &color.to_json());
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("color.rz");
    doc.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
    assert_ne!(&bytes[8..12], &95u32.to_le_bytes(), "no word 95");
}

// ------------------------------------------------------------------ .rz 95 --

#[test]
fn the_byte_word_95_round_trips_and_refuses_both_ways() {
    let dir = tempfile::TempDir::new().unwrap();
    let plain_path = dir.path().join("plain.rz");
    styled(&rect_doc(), 1, "{\"effects\":[{\"type\":\"stroke\"}]}")
        .save_native(plain_path.to_str().unwrap())
        .unwrap();
    let plain = std::fs::read(&plain_path).unwrap();
    assert_ne!(
        &plain[8..12],
        &95u32.to_le_bytes(),
        "no 95 without a pattern stroke"
    );
    for (name, style) in [
        ("pattern", stroke("center", json!({"angle": 30}))),
        (
            "kept",
            json!({"effects":[{"type":"stroke","pattern_fill":{"scale":2}}]}),
        ),
    ] {
        let doc = styled(&rect_doc(), 1, &style.to_string());
        let path = dir.path().join(format!("{name}.rz"));
        doc.save_native(path.to_str().unwrap()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
        assert_eq!(
            &bytes[8..12],
            &95u32.to_le_bytes(),
            "{name}: FIRST inside 45"
        );
        assert_eq!(
            &bytes[12..16],
            &plain[8..12],
            "{name}: then the plain file's word"
        );
        let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            reopened.layers[1].style.as_deref().map(LayerStyle::to_json),
            doc.layers[1].style.as_deref().map(LayerStyle::to_json)
        );
        assert_eq!(
            reopened.flattened().unwrap(),
            doc.flattened().unwrap(),
            "{name}: renders equal"
        );
        let mut stripped = bytes.clone();
        stripped.drain(8..12);
        let cut = dir.path().join(format!("{name}-cut.rz"));
        std::fs::write(&cut, &stripped).unwrap();
        let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
            panic!("{name}: a pattern stroke without the word must refuse");
        };
        assert!(
            message.ends_with("native pattern strokes require DRDC 95"),
            "{message}"
        );
    }
    let mut forged = plain.clone();
    forged.splice(8..8, 95u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = DrDocument::open(path.to_str().unwrap()) else {
        panic!("the word without a pattern stroke must refuse");
    };
    assert!(
        message.ends_with("invalid native pattern stroke word"),
        "{message}"
    );
}
