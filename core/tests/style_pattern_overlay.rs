//! Pattern Overlay: the JSON, the FOLLOW anchor, the
//! PYRAMID scale model, the render under other effects, Blend Interior as
//! Group, the comps payload and the cache. The oracle is an f64 PYRAMID
//! sampler written here from `pattern_tile`'s module doc (periodic 2 × 2
//! halvings, an odd side over two periods, corner-aligned bilinear, wrapping) fed through the straight
//! Normal composite; take1's S37/S50/S200 TIFFs, take2's scales and odd
//! sides and take3's held-out odd sides from `~/devel/doctored-fixtures`
//! check it against Photoshop when present.
//! Every style is applied through `dr_doc_set_layer_style`. Never a golden.

use std::ffi::CString;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_comps::{CompCaptures, CompRequest};
use doctored_core::ffi_style::dr_pattern_presets_json;
use doctored_core::style::LayerStyle;
use image::RgbaImage;
use serde_json::Value;

use crate::common;
use common::*;

// ---------------------------------------------------------------- helpers --

const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (13, 9, 50, 40);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];

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
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A tile: `(w, h, straight RGBA8)`.
type Tile = (u32, u32, Vec<u8>);

/// take1's tile T: 12 x 8, opaque, (20x + 10, 30y + 15, 200 | 60).
fn tile_t() -> Tile {
    tile_of(12, 8, |x, y| {
        [
            (20 * x + 10) as u8,
            (30 * y + 15) as u8,
            if (x + y) % 2 == 0 { 200 } else { 60 },
            255,
        ]
    })
}

fn tile_of(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Tile {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&f(x, y));
        }
    }
    (w, h, rgba)
}

fn custom(tile: &Tile, name: &str) -> String {
    format!(
        "{{\"name\":\"{name}\",\"width\":{},\"height\":{},\"rgba\":\"{}\"}}",
        tile.0,
        tile.1,
        base64(&tile.2)
    )
}

/// A style holding one pattern overlay with `pattern` (a JSON object) and
/// `extra` spliced into the effect.
fn overlay(pattern: &str, extra: &str) -> String {
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!(",{extra}")
    };
    format!("{{\"effects\":[{{\"type\":\"pattern_overlay\",\"pattern\":{pattern}{extra}}}]}}")
}

fn rect_doc() -> DrDocument {
    rect_layer_doc(CANVAS, RECT, GREEN)
}

/// The f64 PYRAMID oracle (module doc): premultiplied unit levels.
struct Level {
    w: usize,
    h: usize,
    px: Vec<[f64; 4]>,
    sx: f64,
    sy: f64,
}

fn level_of(tile: &Tile, scale: f64) -> Level {
    let (w0, h0) = (tile.0 as usize, tile.1 as usize);
    let mut px: Vec<[f64; 4]> = tile
        .2
        .chunks(4)
        .map(|p| {
            let a = f64::from(p[3]) / 255.0;
            [
                f64::from(p[0]) / 255.0 * a,
                f64::from(p[1]) / 255.0 * a,
                f64::from(p[2]) / 255.0 * a,
                a,
            ]
        })
        .collect();
    let (mut w, mut h) = (w0, h0);
    // The period each axis's level spans, in source texels: an odd side is
    // tiled twice before its halving (ODD-DOUBLE), so it keeps its texels.
    let (mut pw, mut ph) = (w0 as f64, h0 as f64);
    let halvings = ((1.0 / scale).log2().floor()).max(0.0) as u32;
    let mean = |a: [f64; 4], b: [f64; 4]| -> [f64; 4] {
        [
            (a[0] + b[0]) / 2.0,
            (a[1] + b[1]) / 2.0,
            (a[2] + b[2]) / 2.0,
            (a[3] + b[3]) / 2.0,
        ]
    };
    for _ in 0..halvings {
        let nw = if w % 2 == 0 { w / 2 } else { w };
        if w % 2 == 1 {
            pw *= 2.0;
        }
        let mut next = vec![[0.0; 4]; nw * h];
        for y in 0..h {
            for j in 0..nw {
                next[y * nw + j] = mean(px[y * w + (2 * j) % w], px[y * w + (2 * j + 1) % w]);
            }
        }
        px = next;
        w = nw;
        let nh = if h % 2 == 0 { h / 2 } else { h };
        if h % 2 == 1 {
            ph *= 2.0;
        }
        let mut next = vec![[0.0; 4]; w * nh];
        for x in 0..w {
            for j in 0..nh {
                next[j * w + x] = mean(px[((2 * j) % h) * w + x], px[((2 * j + 1) % h) * w + x]);
            }
        }
        px = next;
        h = nh;
    }
    Level {
        w,
        h,
        px,
        sx: scale * pw / w as f64,
        sy: scale * ph / h as f64,
    }
}

/// Straight unit RGBA at canvas pixel (x, y) with the tile's canvas origin
/// `o`: corner-aligned bilinear, wrapping.
fn sample(l: &Level, o: (f64, f64), x: f64, y: f64) -> [f64; 4] {
    let (u, v) = ((x - o.0) / l.sx, (y - o.1) / l.sy);
    let (i0, j0) = (u.floor(), v.floor());
    let (fu, fv) = (u - i0, v - j0);
    let mut acc = [0.0; 4];
    for (di, dj, k) in [
        (0.0, 0.0, (1.0 - fu) * (1.0 - fv)),
        (1.0, 0.0, fu * (1.0 - fv)),
        (0.0, 1.0, (1.0 - fu) * fv),
        (1.0, 1.0, fu * fv),
    ] {
        let i = ((i0 + di) as i64).rem_euclid(l.w as i64) as usize;
        let j = ((j0 + dj) as i64).rem_euclid(l.h as i64) as usize;
        for (slot, v) in acc.iter_mut().zip(l.px[j * l.w + i]) {
            *slot += k * v;
        }
    }
    if acc[3] <= 0.0 {
        return [0.0; 4];
    }
    [acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3], acc[3]]
}

/// The Normal composite of the sample over an opaque `under`, at `opacity`,
/// with the coverage quantized as the renderer stores it.
fn over(s: [f64; 4], under: [u8; 4], opacity: f64) -> [u8; 4] {
    let a = (s[3] * 255.0).round() / 255.0 * opacity;
    let mut out = [255u8; 4];
    for c in 0..3 {
        out[c] = (s[c] * 255.0 * a + f64::from(under[c]) * (1.0 - a)).round() as u8;
    }
    out
}

/// Asserts the flattened `doc`: inside `rect`, the oracle at the tile's
/// canvas origin `o`; outside, white. ±1 colour, alpha exact.
fn assert_tiled(
    doc: &DrDocument,
    rect: (i32, i32, u32, u32),
    l: &Level,
    o: (f64, f64),
    what: &str,
) {
    let flat = doc.flattened().unwrap();
    for y in 0..doc.height {
        for x in 0..doc.width {
            let got = flat.get_pixel(x, y).0;
            let inside = (x as i32) >= rect.0
                && (y as i32) >= rect.1
                && (x as i32) < rect.0 + rect.2 as i32
                && (y as i32) < rect.1 + rect.3 as i32;
            let want = if inside {
                over(sample(l, o, f64::from(x), f64::from(y)), GREEN, 1.0)
            } else {
                WHITE
            };
            assert_close(&got, &want, &format!("{what} ({x},{y})"));
        }
    }
}

fn echoed(doc: &DrDocument, idx: usize) -> Value {
    serde_json::from_str(&ffi_style(doc as *const DrDocument, idx).expect("a style")).unwrap()
}

fn effect0(doc: &DrDocument, idx: usize) -> Value {
    echoed(doc, idx)["effects"][0].clone()
}

// ------------------------------------------------------------------- JSON --

#[test]
fn the_defaults_and_the_canonical_echo() {
    let style = parse("{\"effects\":[{\"type\":\"pattern_overlay\"}]}");
    let json: Value = serde_json::from_str(&style.to_json()).unwrap();
    let e = &json["effects"][0];
    assert_eq!(e["type"], "pattern_overlay");
    assert_eq!(e["blend"], "normal");
    assert_eq!(e["opacity"], 1.0);
    assert_eq!(e["scale"], 1.0);
    assert_eq!(e["link_with_layer"], true);
    assert_eq!(e["phase"], serde_json::json!([0.0, 0.0]));
    assert_eq!(e["pattern"], serde_json::json!({"preset": "checkerboard"}));
    assert!(e.get("anchor").is_none(), "anchor is written only when set");
    assert_eq!(parse(&style.to_json()), style, "the echo re-parses equal");
}

#[test]
fn a_custom_tile_round_trips_with_one_form_and_sorted_keys() {
    let t = tile_t();
    let style = parse(&overlay(
        &custom(&t, "T"),
        "\"scale\":0.37,\"phase\":[5,2],\"anchor\":[3,4]",
    ));
    let text = style.to_json();
    let e = &serde_json::from_str::<Value>(&text).unwrap()["effects"][0];
    let p = e["pattern"].as_object().unwrap();
    assert_eq!(
        p.keys().cloned().collect::<Vec<_>>(),
        ["height", "name", "rgba", "width"],
        "the custom form alone, keys sorted"
    );
    assert_eq!(p["rgba"], base64(&t.2));
    assert_eq!(e["scale"], 0.37);
    assert_eq!(e["anchor"], serde_json::json!([3, 4]));
    assert_eq!(parse(&text), style);
}

#[test]
fn every_strict_refusal_names_its_key() {
    let doc = rect_doc();
    let cases = [
        (
            "{\"preset\":\"plaid\"}",
            "",
            "effects[0].pattern.preset must be one of checkerboard, stripes, dots, grid",
        ),
        (
            "{\"name\":\"x\",\"width\":0,\"height\":2,\"rgba\":\"\"}",
            "",
            "effects[0].pattern.width must be an integer 1..=512",
        ),
        (
            "{\"name\":\"x\",\"width\":2,\"height\":513,\"rgba\":\"\"}",
            "",
            "effects[0].pattern.height must be an integer 1..=512",
        ),
        (
            "{\"name\":\"x\",\"width\":2,\"height\":2,\"rgba\":\"AAAA\"}",
            "",
            "effects[0].pattern.rgba must be base64 of width x height x 4 bytes",
        ),
        (
            "{\"name\":\"x\"}",
            "",
            "effects[0].pattern must hold either preset or rgba",
        ),
        (
            "{\"preset\":\"dots\"}",
            "\"anchor\":[1.5,2]",
            "effects[0].anchor must be two integers",
        ),
        (
            "{\"preset\":\"dots\"}",
            "\"anchor\":[1]",
            "effects[0].anchor must be two integers",
        ),
    ];
    for (pattern, extra, want) in cases {
        let got = style_error(&doc as *const DrDocument, 1, &overlay(pattern, extra));
        assert_eq!(
            got.as_deref(),
            Some(format!("style: {want}").as_str()),
            "{pattern} {extra}"
        );
    }
}

#[test]
fn lenient_parsing_takes_the_default_for_an_unknown_preset() {
    let lenient = LayerStyle::from_json_lenient(&overlay("{\"preset\":\"plaid\"}", "")).unwrap();
    assert_eq!(
        lenient,
        parse(&overlay("{\"preset\":\"checkerboard\"}", ""))
    );
}

#[test]
fn the_presets_export_matches_their_frozen_definitions() {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_pattern_presets_json(&mut err) };
    assert!(!raw.is_null() && err.is_null());
    let text = unsafe { CString::from_raw(raw) }.into_string().unwrap();
    let list: Vec<Value> = serde_json::from_str(&text).unwrap();
    let ids: Vec<&str> = list.iter().map(|p| p["preset"].as_str().unwrap()).collect();
    assert_eq!(ids, ["checkerboard", "stripes", "dots", "grid"]);
    let want: [(&str, Tile); 4] = [
        (
            "Checkerboard",
            tile_of(16, 16, |x, y| {
                if (x / 8 + y / 8) % 2 == 0 {
                    [255, 255, 255, 255]
                } else {
                    [192, 192, 192, 255]
                }
            }),
        ),
        (
            "Stripes",
            tile_of(8, 8, |x, _| if x < 4 { [0, 0, 0, 255] } else { [255; 4] }),
        ),
        (
            "Dots",
            tile_of(12, 12, |x, y| {
                let (dx, dy) = (f64::from(x) + 0.5 - 6.0, f64::from(y) + 0.5 - 6.0);
                if dx * dx + dy * dy <= 9.0 {
                    [0, 0, 0, 255]
                } else {
                    [0, 0, 0, 0]
                }
            }),
        ),
        (
            "Grid",
            tile_of(10, 10, |x, y| {
                if x == 0 || y == 0 {
                    [128, 128, 128, 255]
                } else {
                    [0, 0, 0, 0]
                }
            }),
        ),
    ];
    for (entry, (name, tile)) in list.iter().zip(want) {
        assert_eq!(entry["name"], name);
        assert_eq!(entry["width"], tile.0);
        assert_eq!(entry["height"], tile.1);
        assert_eq!(entry["rgba"], base64(&tile.2), "{name}");
    }
}

// ---------------------------------------------------------------- anchors --

#[test]
fn the_setter_anchors_a_linked_overlay_and_drops_an_unlinked_anchor() {
    let doc = rect_doc();
    let linked = styled(&doc, 1, &overlay("{\"preset\":\"stripes\"}", ""));
    assert_eq!(effect0(&linked, 1)["anchor"], serde_json::json!([13, 9]));
    // Echoing the style back keeps the anchor: no change, a refusal.
    let echo = ffi_style(&linked as *const DrDocument, 1).unwrap();
    assert!(try_styled(&linked, 1, Some(&echo)).unwrap().is_none());
    // A carried anchor is kept even where the layer now sits elsewhere.
    let kept = styled(
        &doc,
        1,
        &overlay("{\"preset\":\"stripes\"}", "\"anchor\":[1,2]"),
    );
    assert_eq!(effect0(&kept, 1)["anchor"], serde_json::json!([1, 2]));
    let unlinked = styled(
        &doc,
        1,
        &overlay(
            "{\"preset\":\"stripes\"}",
            "\"link_with_layer\":false,\"anchor\":[1,2]",
        ),
    );
    assert!(effect0(&unlinked, 1).get("anchor").is_none());
}

#[test]
fn a_linked_tile_starts_at_the_canvas_origin_and_follows_a_move() {
    let t = tile_t();
    let l = level_of(&t, 1.0);
    let doc = styled(&rect_doc(), 1, &overlay(&custom(&t, "T"), ""));
    assert_tiled(&doc, RECT, &l, (0.0, 0.0), "linked at set time");
    let moved = doc.with_layer_offset(1, RECT.0 + 7, RECT.1 + 3).unwrap();
    let rect = (RECT.0 + 7, RECT.1 + 3, RECT.2, RECT.3);
    assert_tiled(&moved, rect, &l, (7.0, 3.0), "linked, moved (+7, +3)");
    let phased = styled(
        &rect_doc(),
        1,
        &overlay(&custom(&t, "T"), "\"phase\":[5,2]"),
    );
    assert_tiled(&phased, RECT, &l, (5.0, 2.0), "linked, phase (5, 2)");
}

#[test]
fn an_unlinked_tile_stays_on_the_canvas_grid() {
    let t = tile_t();
    let l = level_of(&t, 1.0);
    let json = overlay(
        &custom(&t, "T"),
        "\"link_with_layer\":false,\"phase\":[5,2]",
    );
    let doc = styled(&rect_doc(), 1, &json);
    assert_tiled(&doc, RECT, &l, (5.0, 2.0), "unlinked");
    let moved = doc.with_layer_offset(1, RECT.0 + 7, RECT.1 + 3).unwrap();
    let rect = (RECT.0 + 7, RECT.1 + 3, RECT.2, RECT.3);
    assert_tiled(&moved, rect, &l, (5.0, 2.0), "unlinked, moved");
}

#[test]
fn delete_cropped_pixels_rebases_the_anchor_and_keeps_the_picture() {
    let t = tile_t();
    let doc = rect_layer_doc(CANVAS, (-5, -3, 40, 30), GREEN);
    let doc = styled(&doc, 1, &overlay(&custom(&t, "T"), "\"phase\":[1,1]"));
    let before = doc.flattened().unwrap();
    let trimmed = doc
        .delete_cropped_pixels()
        .expect("the layer reaches outside");
    assert_eq!(trimmed.layers[1].offset, (0, 0));
    assert_eq!(effect0(&trimmed, 1)["anchor"], serde_json::json!([0, 0]));
    assert_eq!(
        trimmed.flattened().unwrap(),
        before,
        "the picture is unchanged"
    );
}

#[test]
fn scale_effects_scales_the_scale_phase_and_anchor() {
    let style = parse(&overlay(
        "{\"preset\":\"dots\"}",
        "\"scale\":0.5,\"phase\":[3,-2],\"anchor\":[10,5]",
    ));
    let scaled: Value = serde_json::from_str(&style.scaled(2.0).to_json()).unwrap();
    let e = &scaled["effects"][0];
    assert_eq!(e["scale"], 1.0);
    assert_eq!(e["phase"], serde_json::json!([6.0, -4.0]));
    assert_eq!(e["anchor"], serde_json::json!([20, 10]));
}

// ------------------------------------------------------------------ scale --

#[test]
fn scale_reads_the_pyramid_level_bilinearly() {
    let t = tile_t();
    for scale in [2.0, 1.0, 0.5, 0.37, 0.25, 0.12] {
        let json = overlay(&custom(&t, "T"), &format!("\"scale\":{scale}"));
        let doc = styled(&rect_doc(), 1, &json);
        assert_tiled(
            &doc,
            RECT,
            &level_of(&t, scale),
            (0.0, 0.0),
            &format!("scale {scale}"),
        );
    }
}

#[test]
fn an_odd_tile_halves_over_two_periods() {
    // 11 x 7 at 50 %: ODD-DOUBLE (take2's refit) keeps 11 x 7 texels, each
    // the mean of a pair of the tile tiled twice, read at s' = 1.
    let t2 = tile_of(11, 7, |x, y| {
        [
            (22 * x + 15) as u8,
            (35 * y + 20) as u8,
            if (x + y) % 2 == 0 { 200 } else { 60 },
            255,
        ]
    });
    let doc = styled(
        &rect_doc(),
        1,
        &overlay(&custom(&t2, "T2"), "\"scale\":0.5"),
    );
    let l = level_of(&t2, 0.5);
    assert_eq!((l.w, l.h), (11, 7));
    assert_eq!((l.sx, l.sy), (1.0, 1.0));
    assert_tiled(&doc, RECT, &l, (0.0, 0.0), "T2 at 50 %");
}

/// take `n`'s RGB8 job folder.
fn take_folder(n: u32) -> Option<PathBuf> {
    let root: PathBuf = std::env::var_os("HOME")?.into();
    Some(root.join(format!(
        "devel/doctored-fixtures/photoshop/pattern-overlay-557/take{n}/jobs/rgb8"
    )))
}

fn take1_folder() -> Option<PathBuf> {
    take_folder(1)
}

/// take1's TIFF of `case` (RGB8), as 8-bit codes, when the fixture exists.
fn take1(case: &str) -> Option<RgbaImage> {
    take_tiff(1, case)
}

/// take `n`'s TIFF of `case` (RGB8), as 8-bit codes, when it exists.
fn take_tiff(n: u32, case: &str) -> Option<RgbaImage> {
    let folder = take_folder(n)?;
    let path = std::fs::read_dir(&folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(case) && n.ends_with(".tif"))
        })?;
    let doc = DrDocument::open(path.to_str()?).ok()?;
    doc.flattened().ok()
}

#[test]
fn scale_matches_photoshops_take1_within_two_codes() {
    let t = tile_t();
    let mut checked = 0;
    for (case, scale) in [("s50_rgb8", 0.5), ("s200_rgb8", 2.0), ("s37_rgb8", 0.37)] {
        let Some(ps) = take1(case) else {
            eprintln!("skipping {case}: take1 fixture absent");
            continue;
        };
        let doc = styled(
            &rect_doc(),
            1,
            &overlay(&custom(&t, "T"), &format!("\"scale\":{scale}")),
        );
        let ours = doc.flattened().unwrap();
        let mut worst = 0i32;
        for y in RECT.1 as u32..(RECT.1 as u32 + RECT.3) {
            for x in RECT.0 as u32..(RECT.0 as u32 + RECT.2) {
                let (a, b) = (ours.get_pixel(x, y).0, ps.get_pixel(x, y).0);
                for c in 0..3 {
                    worst = worst.max((i32::from(a[c]) - i32::from(b[c])).abs());
                }
            }
        }
        assert!(worst <= 2, "{case}: max {worst} codes from Photoshop");
        checked += 1;
    }
    if take1_folder().is_some_and(|f| f.is_dir()) {
        assert_eq!(checked, 3, "every take1 case present is compared");
    }
}

/// take2's 11 x 7 tile T2: (22x + 15, 35y + 20, 200 | 60), opaque.
fn tile_t2() -> Tile {
    tile_of(11, 7, |x, y| {
        [
            (22 * x + 15) as u8,
            (35 * y + 20) as u8,
            if (x + y) % 2 == 0 { 200 } else { 60 },
            255,
        ]
    })
}

#[test]
fn scale_and_odd_sides_match_photoshops_take2_within_two_codes() {
    // take2's held-out scales and its odd cases (the ODD-DOUBLE refit),
    // linked at the canvas origin, never moved (P5's anchor time is an open
    // item, so it is not here).
    let cases: [(&str, Tile, f64); 6] = [
        ("s25_rgb8", tile_t(), 0.25),
        ("s70_rgb8", tile_t(), 0.70),
        ("s150_rgb8", tile_t(), 1.50),
        ("s333_rgb8", tile_t(), 3.33),
        ("s12_rgb8", tile_t(), 0.12),
        ("t2s50_rgb8", tile_t2(), 0.50),
    ];
    let mut checked = 0;
    for (case, tile, scale) in cases {
        let Some(ps) = take_tiff(2, case) else {
            continue;
        };
        let json = overlay(&custom(&tile, "T"), &format!("\"scale\":{scale}"));
        let ours = styled(&rect_doc(), 1, &json).flattened().unwrap();
        let mut worst = 0i32;
        for y in RECT.1 as u32..(RECT.1 as u32 + RECT.3) {
            for x in RECT.0 as u32..(RECT.0 as u32 + RECT.2) {
                let (a, b) = (ours.get_pixel(x, y).0, ps.get_pixel(x, y).0);
                for c in 0..3 {
                    worst = worst.max((i32::from(a[c]) - i32::from(b[c])).abs());
                }
            }
        }
        assert!(worst <= 2, "{case}: max {worst} codes from Photoshop");
        checked += 1;
    }
    if take_folder(2).is_some_and(|f| f.is_dir()) {
        assert_eq!(checked, 6, "every take2 case present is compared");
    }
}

#[test]
fn odd_sides_match_photoshops_take3_within_two_codes() {
    // take3's held-out odd tiles and scales (the ODD-DOUBLE confirmation):
    // T3 9 x 5 and T4 13 x 10, each at two scales, and T2 at 30 %.
    let t3 = tile_of(9, 5, |x, y| {
        [
            (25 * x + 20) as u8,
            (45 * y + 25) as u8,
            if (x + y) % 2 == 0 { 200 } else { 60 },
            255,
        ]
    });
    let t4 = tile_of(13, 10, |x, y| {
        [
            (18 * x + 12) as u8,
            (22 * y + 18) as u8,
            if (x + y) % 2 == 0 { 200 } else { 60 },
            255,
        ]
    });
    let cases: [(&str, Tile, f64); 5] = [
        ("t3s50_rgb8", t3.clone(), 0.50),
        ("t3s22_rgb8", t3, 0.22),
        ("t4s45_rgb8", t4.clone(), 0.45),
        ("t4s15_rgb8", t4, 0.15),
        ("t2s30_rgb8", tile_t2(), 0.30),
    ];
    let mut checked = 0;
    for (case, tile, scale) in cases {
        let Some(ps) = take_tiff(3, case) else {
            continue;
        };
        let json = overlay(&custom(&tile, "T"), &format!("\"scale\":{scale}"));
        let ours = styled(&rect_doc(), 1, &json).flattened().unwrap();
        let mut worst = 0i32;
        for y in RECT.1 as u32..(RECT.1 as u32 + RECT.3) {
            for x in RECT.0 as u32..(RECT.0 as u32 + RECT.2) {
                let (a, b) = (ours.get_pixel(x, y).0, ps.get_pixel(x, y).0);
                for c in 0..3 {
                    worst = worst.max((i32::from(a[c]) - i32::from(b[c])).abs());
                }
            }
        }
        assert!(worst <= 2, "{case}: max {worst} codes from Photoshop");
        checked += 1;
    }
    if take_folder(3).is_some_and(|f| f.is_dir()) {
        assert_eq!(checked, 5, "every take3 odd case present is compared");
    }
}

// ------------------------------------------------------------------ render --

#[test]
fn a_color_overlay_draws_over_the_pattern() {
    let t = tile_t();
    let l = level_of(&t, 1.0);
    let json = format!(
        "{{\"effects\":[{{\"type\":\"pattern_overlay\",\"pattern\":{}}},\
         {{\"type\":\"color_overlay\",\"color\":\"#0000ff\",\"opacity\":0.5}}]}}",
        custom(&t, "T")
    );
    let doc = styled(&rect_doc(), 1, &json);
    let flat = doc.flattened().unwrap();
    for (x, y) in [(13u32, 9u32), (20, 15), (40, 30), (62, 48)] {
        let pattern = over(
            sample(&l, (0.0, 0.0), f64::from(x), f64::from(y)),
            GREEN,
            1.0,
        );
        let want = over([0.0, 0.0, 1.0, 1.0], pattern, 0.5);
        assert_close(&flat.get_pixel(x, y).0, &want, &format!("({x},{y})"));
    }
}

#[test]
fn a_transparent_texel_shows_the_layer() {
    let doc = styled(&rect_doc(), 1, &overlay("{\"preset\":\"dots\"}", ""));
    let flat = doc.flattened().unwrap();
    // Dots: the disc's centre texel (6, 6) of each 12-px cell is black, its
    // corner (0, 0) transparent; the tile starts at the canvas origin.
    assert_eq!(flat.get_pixel(18, 18).0, [0, 0, 0, 255]);
    assert_eq!(flat.get_pixel(24, 24).0, GREEN);
}

#[test]
fn blend_interior_as_group_applies_the_layers_mode_to_the_pattern() {
    let t = tile_t();
    let l = level_of(&t, 1.0);
    let backdrop = [200u8, 100, 50, 255];
    let doc = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, backdrop))
        .adding_image_layer(0, solid(RECT.2, RECT.3, GREEN), "Rect")
        .unwrap();
    let mut doc = doc.with_layer_offset(1, RECT.0, RECT.1).unwrap();
    // Made at RECT (`Layer::reference`).
    doc.layers[1].reference = (0, 0);
    doc.layers[1].blend = doctored_core::doc::BlendMode::Multiply;
    let off = styled(&doc, 1, &overlay(&custom(&t, "T"), ""));
    let on = styled(
        &doc,
        1,
        &format!(
            "{{\"blend_interior_as_group\":true,\"effects\":[{{\"type\":\"pattern_overlay\",\"pattern\":{}}}]}}",
            custom(&t, "T")
        ),
    );
    let (f_off, f_on) = (off.flattened().unwrap(), on.flattened().unwrap());
    for (x, y) in [(13u32, 9u32), (30, 20), (50, 40)] {
        let s = sample(&l, (0.0, 0.0), f64::from(x), f64::from(y));
        let code = |c: usize| (s[c] * 255.0).round();
        // OFF: the pattern itself, Normal over the multiplied layer.
        let want_off = [code(0) as u8, code(1) as u8, code(2) as u8, 255];
        // ON: the unit (the pattern over the layer) multiplied onto the backdrop.
        let mul = |c: usize| (code(c) * f64::from(backdrop[c]) / 255.0).round() as u8;
        let want_on = [mul(0), mul(1), mul(2), 255];
        assert_close(&f_off.get_pixel(x, y).0, &want_off, "flag off");
        assert_close(&f_on.get_pixel(x, y).0, &want_on, "flag on");
    }
}

#[test]
fn opacity_zero_draws_nothing() {
    let plain = rect_doc().flattened().unwrap();
    let doc = styled(
        &rect_doc(),
        1,
        &overlay("{\"preset\":\"stripes\"}", "\"opacity\":0"),
    );
    assert_eq!(doc.flattened().unwrap(), plain);
}

// ------------------------------------------------------------------- comps --

fn noise_tile(seed: u32) -> Tile {
    // A per-texel integer hash: distinct bytes everywhere, no state.
    tile_of(512, 512, |x, y| {
        let mut h = (x * 512 + y)
            .wrapping_mul(2654435761)
            .wrapping_add(seed.wrapping_mul(40503));
        h ^= h >> 15;
        h = h.wrapping_mul(2246822519);
        h ^= h >> 13;
        let b = h.to_le_bytes();
        [b[0], b[1], b[2], 255]
    })
}

#[test]
fn a_512_tile_round_trips_in_a_comp_and_too_many_hit_the_payload_cap() {
    let dir = tempfile::TempDir::new().unwrap();
    let t = noise_tile(1);
    let json = overlay(&custom(&t, "Noise"), "");
    let doc = styled(&rect_doc(), 1, &json);
    let save = CompRequest::Save {
        name: "Patterned".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = doc.edit_layer_comps(&save).unwrap().unwrap();
    let path = dir.path().join("comp.rz");
    saved.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    let cleared = try_styled(&reopened, 1, None).unwrap().unwrap();
    let applied = cleared
        .edit_layer_comps(&CompRequest::Apply { comp: 0 })
        .unwrap()
        .unwrap();
    assert_eq!(
        applied.layers[1].style.as_deref().map(LayerStyle::to_json),
        doc.layers[1].style.as_deref().map(LayerStyle::to_json),
        "the comp restores the 512² tile"
    );
    // Seven layers of distinct 1 MiB tiles: ~9.8 MiB of base64 in one comp.
    let mut many = DrDocument::from_pixels(solid(8, 8, WHITE));
    for i in 0..7u32 {
        many = many
            .adding_image_layer(many.layers.len() - 1, solid(4, 4, GREEN), "L")
            .unwrap();
        let top = many.layers.len() - 1;
        many = styled(&many, top, &overlay(&custom(&noise_tile(10 + i), "N"), ""));
    }
    let Err(message) = many.edit_layer_comps(&save) else {
        panic!("seven 512² tiles must pass the 8 MiB comps payload");
    };
    assert_eq!(message, "layer comps exceed the .doctored payload cap");
}

// ------------------------------------------------------------------- cache --

#[test]
fn an_opacity_edit_restamps_and_a_scale_edit_equals_a_full_render() {
    let t = tile_t();
    let v1 = styled(&rect_doc(), 1, &overlay(&custom(&t, "T"), "\"opacity\":1"));
    let _ = v1.flattened().unwrap();
    let echo = |doc: &DrDocument, key: &str, value: &str| {
        let mut e = effect0(doc, 1);
        e[key] = serde_json::from_str(value).unwrap();
        serde_json::json!({"effects": [e]}).to_string()
    };
    let v2 = styled(&v1, 1, &echo(&v1, "opacity", "0.5"));
    let fresh = |doc: &DrDocument| {
        let mut copy = doc.clone();
        let json = copy.layers[1].style.as_deref().unwrap().to_json();
        copy.layers[1].style = Some(std::sync::Arc::new(parse(&json)));
        copy.flattened().unwrap()
    };
    assert_eq!(v2.flattened().unwrap(), fresh(&v2), "the restamped opacity");
    let v3 = styled(&v2, 1, &echo(&v2, "scale", "0.37"));
    assert_eq!(
        v3.flattened().unwrap(),
        fresh(&v3),
        "a new scale renders fresh"
    );
    let l = level_of(&t, 0.37);
    let flat = v3.flattened().unwrap();
    let want = over(sample(&l, (0.0, 0.0), 30.0, 20.0), GREEN, 0.5);
    assert_close(
        &flat.get_pixel(30, 20).0,
        &want,
        "scale 0.37 at half opacity",
    );
}

// ------------------------------------------------------------------ .rz 90 --

#[test]
fn the_byte_word_90_round_trips_and_refuses_both_ways() {
    let dir = tempfile::TempDir::new().unwrap();
    let t = tile_t();
    let doc = styled(&rect_doc(), 1, &overlay(&custom(&t, "T"), "\"scale\":0.37"));
    let path = dir.path().join("p.rz");
    doc.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
    assert_eq!(
        &bytes[8..12],
        &90u32.to_le_bytes(),
        "the FIRST word inside 45"
    );
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        reopened.layers[1].style.as_deref().map(LayerStyle::to_json),
        doc.layers[1].style.as_deref().map(LayerStyle::to_json)
    );
    assert_eq!(reopened.flattened().unwrap(), doc.flattened().unwrap());
    // The pattern without the word, and the word without a pattern.
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
        panic!("a pattern without the word must refuse");
    };
    assert!(
        message.ends_with("native pattern overlays require DRDC 90"),
        "{message}"
    );
    let plain_path = dir.path().join("plain.rz");
    rect_doc()
        .save_native(plain_path.to_str().unwrap())
        .unwrap();
    let mut plain = std::fs::read(&plain_path).unwrap();
    assert_ne!(
        &plain[8..12],
        &90u32.to_le_bytes(),
        "no word without a pattern"
    );
    plain.splice(8..8, 90u32.to_le_bytes());
    let forged = dir.path().join("forged.rz");
    std::fs::write(&forged, &plain).unwrap();
    let Err(message) = DrDocument::open(forged.to_str().unwrap()) else {
        panic!("the word without a pattern must refuse");
    };
    assert!(
        message.ends_with("invalid native pattern overlay word"),
        "{message}"
    );
}

#[test]
fn a_comp_only_pattern_claims_the_word() {
    // The pattern lives only in a comp's recorded appearance: the layer's
    // own style was cleared after the comp was saved.
    let dir = tempfile::TempDir::new().unwrap();
    let doc = styled(&rect_doc(), 1, &overlay("{\"preset\":\"grid\"}", ""));
    let save = CompRequest::Save {
        name: "Grid".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = doc.edit_layer_comps(&save).unwrap().unwrap();
    let cleared = try_styled(&saved, 1, None).unwrap().unwrap();
    assert!(cleared.layers[1].style.is_none());
    let path = dir.path().join("comp.rz");
    cleared.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &90u32.to_le_bytes());
    assert!(DrDocument::open(path.to_str().unwrap()).is_ok());
}
