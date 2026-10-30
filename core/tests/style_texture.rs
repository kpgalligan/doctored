//! The Bevel & Emboss TEXTURE at 8 bits, black-box through
//! `dr_doc_set_layer_style` and the projection: the `texture_enabled` and
//! `texture` keys of the style JSON, an untextured bevel's bytes, the
//! texture on the flat interior against an oracle written here, Photoshop's
//! own readings from noise take 2 and texture take 3
//! (`fixtures/psd_photoshop/texture/rows.json`, `texture_take.py
//! --export-fixture`), the anchor rules, Scale Effects and the byte `.rz`
//! word 98.
//!
//! The scene is the takes' `dark`: a 128 x 96 black canvas under a black
//! 80 x 64 layer made at (24, 16), an Inner Bevel of size 16, depth 1, angle
//! 0, altitude 30 drawn twice — `H` with a white Normal highlight at 100 %
//! and the shadow at 0 %, `S` the reverse — so a pixel's red IS the code.
//! FLAT is where the untextured bevel reads H = S = 0 at angle 0 AND at
//! angle 90, inside the shape: neither base slope shades there.
//!
//! The oracle (this file's own restatement of the take's REFIT, not the
//! crate's code): `t` = Photoshop's Gray `0.30 r + 0.59 g + 0.11 b` of
//! take1's tile T, `T(x, y) = (20x + 10, 30y + 15, 200 if x + y is even else
//! 60)`, at `((X − Ox) mod 12, (Y − Oy) mod 8)`, O the tile's canvas origin
//! (the layer's offset when linked, else 0, plus the phase), over 255; the
//! slopes `hx = g (t[x+1] − t[x−1]) / 2` and the same in y, `g` = −16.1 ×
//! depth × (−1 for Invert) × the bevel's depth × its size / 16 (both 1 on
//! the scene; take 3's coupling); `nl = (−hx lx − hy ly + lz) / |(−hx, −hy, 1)|`,
//! `d = (nl − lz) / (1 − lz)` where `nl >= lz`, else `(nl − lz) / lz`;
//! `H = round(255 clamp(d))`, `S = round(255 clamp(−d))`.

use std::path::PathBuf;

use doctored_core::doc::DrDocument;
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use crate::common::{parse, solid, styled, try_styled};

const CANVAS: (u32, u32) = (128, 96);
const RECT: (i32, i32, u32, u32) = (24, 16, 80, 64);
const BLACK: [u8; 4] = [0, 0, 0, 255];
const G: f64 = -16.1;

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
fn texel(x: u32, y: u32) -> [u8; 3] {
    [
        (20 * x + 10) as u8,
        (30 * y + 15) as u8,
        if (x + y).is_multiple_of(2) { 200 } else { 60 },
    ]
}

fn tile_json() -> Value {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..12 {
            rgba.extend_from_slice(&texel(x, y));
            rgba.push(255);
        }
    }
    json!({"name":"T","width":12,"height":8,"rgba":base64(&rgba)})
}

/// A black canvas of `canvas` under a black layer of `rect` made there.
fn scene(canvas: (u32, u32), rect: (i32, i32, u32, u32)) -> DrDocument {
    let doc = DrDocument::from_pixels(solid(canvas.0, canvas.1, BLACK))
        .adding_image_layer(0, solid(rect.2, rect.3, BLACK), "Rect")
        .unwrap();
    let mut doc = doc.with_layer_offset(1, rect.0, rect.1).unwrap();
    doc.layers[1].reference = (0, 0);
    doc
}

fn dark() -> DrDocument {
    scene(CANVAS, RECT)
}

/// The takes' bevel, one side lit (`h` the highlight, else the shadow),
/// `extra` merged into the effect.
fn bevel(h: bool, extra: Value) -> Value {
    let mut e = json!({"type":"bevel_emboss","style":"inner_bevel","depth":1,"direction":"up",
        "size":16,"soften":0,"angle":0,"use_global_light":false,"altitude":30,
        "highlight_blend":"normal","highlight_color":"#ffffff",
        "highlight_opacity": if h {1} else {0},"shadow_blend":"normal","shadow_color":"#ffffff",
        "shadow_opacity": if h {0} else {1}});
    for (k, v) in extra.as_object().unwrap() {
        e[k] = v.clone();
    }
    json!({"effects":[e]})
}

fn textured(depth: f64, invert: bool, link: bool) -> Value {
    json!({"texture_enabled":true,"texture":{"pattern":tile_json(),"depth":depth,
        "invert":invert,"link_with_layer":link}})
}

/// The red plane of `doc` with `style` on layer 1.
fn reds(doc: &DrDocument, style: &Value) -> Vec<u8> {
    styled(doc, 1, &style.to_string())
        .flattened()
        .unwrap()
        .pixels()
        .map(|p| p.0[0])
        .collect()
}

/// (H, S) red planes of `doc` with the bevel and `extra`.
fn hs(doc: &DrDocument, extra: &Value) -> (Vec<u8>, Vec<u8>) {
    (
        reds(doc, &bevel(true, extra.clone())),
        reds(doc, &bevel(false, extra.clone())),
    )
}

/// FLAT (module doc) of a scene whose shape is `rect`.
fn flat(doc: &DrDocument, rect: (i32, i32, u32, u32), canvas_w: u32) -> Vec<usize> {
    let (h0, s0) = hs(doc, &json!({}));
    let (h90, s90) = hs(doc, &json!({"angle":90}));
    (0..h0.len())
        .filter(|&i| {
            let (x, y) = ((i as u32 % canvas_w) as i32, (i as u32 / canvas_w) as i32);
            x > rect.0
                && x < rect.0 + rect.2 as i32 - 1
                && y > rect.1
                && y < rect.1 + rect.3 as i32 - 1
                && h0[i] == 0
                && s0[i] == 0
                && h90[i] == 0
                && s90[i] == 0
        })
        .collect()
}

fn gray(c: [u8; 3]) -> f64 {
    (0.30 * f64::from(c[0]) + 0.59 * f64::from(c[1]) + 0.11 * f64::from(c[2])) / 255.0
}

/// The oracle's (H, S) at canvas (x, y) (module doc).
fn oracle(o: (i32, i32), x: i32, y: i32, gain: f64, light: (f64, f64)) -> (u8, u8) {
    let t = |x: i32, y: i32| {
        gray(texel(
            (x - o.0).rem_euclid(12) as u32,
            (y - o.1).rem_euclid(8) as u32,
        ))
    };
    let hx = gain * (t(x + 1, y) - t(x - 1, y)) / 2.0;
    let hy = gain * (t(x, y + 1) - t(x, y - 1)) / 2.0;
    let (a, alt) = (light.0.to_radians(), light.1.to_radians());
    let (lx, ly, lz) = (alt.cos() * a.cos(), -alt.cos() * a.sin(), alt.sin());
    let nl = (-hx * lx - hy * ly + lz) / (1.0 + hx * hx + hy * hy).sqrt();
    let d = if nl >= lz {
        (nl - lz) / (1.0 - lz)
    } else {
        (nl - lz) / lz
    };
    (
        (255.0 * d.clamp(0.0, 1.0)).round() as u8,
        (255.0 * (-d).clamp(0.0, 1.0)).round() as u8,
    )
}

fn effect0(doc: &DrDocument) -> Value {
    let style: Value =
        serde_json::from_str(&doc.layers[1].style.as_deref().unwrap().to_json()).unwrap();
    style["effects"][0].clone()
}

// ------------------------------------------------------------------- JSON --

#[test]
fn texture_json_parses_clamps_quantizes_and_writes_only_when_carried() {
    let effect = |extra: Value| -> Value {
        let mut e = json!({"type":"bevel_emboss"});
        for (k, v) in extra.as_object().unwrap() {
            e[k] = v.clone();
        }
        let style = parse(&json!({ "effects": [e] }).to_string());
        let v: Value = serde_json::from_str(&style.to_json()).unwrap();
        v["effects"][0].clone()
    };
    let plain = effect(json!({}));
    assert!(plain.get("texture").is_none() && plain.get("texture_enabled").is_none());
    let on = effect(json!({"texture_enabled": true}));
    assert_eq!(on["texture_enabled"], json!(true));
    assert_eq!(
        on["texture"],
        json!({"pattern":{"preset":"checkerboard"},"scale":1.0,"link_with_layer":true,
            "phase":[0.0,0.0],"depth":1.0,"invert":false})
    );
    let t = effect(json!({"texture":{"depth":25,"scale":0.001,"angle":30,"invert":true}}));
    assert!(t.get("texture_enabled").is_none(), "off, yet carried: {t}");
    assert_eq!(t["texture"]["depth"], json!(10.0));
    assert_eq!(t["texture"]["scale"], json!(0.01));
    assert!(
        t["texture"].get("angle").is_none(),
        "angle is not a texture key"
    );
    assert_eq!(
        effect(json!({"texture":{"depth":-30}}))["texture"]["depth"],
        json!(-10.0)
    );
    assert_eq!(
        effect(json!({"texture":{"depth":0.123456}}))["texture"]["depth"],
        json!(0.1235)
    );
    // An angle is parsed and discarded: equal to the same texture without.
    assert_eq!(
        parse(
            &json!({"effects":[{"type":"bevel_emboss","texture":{"angle":45,"depth":2}}]})
                .to_string()
        ),
        parse(&json!({"effects":[{"type":"bevel_emboss","texture":{"depth":2}}]}).to_string())
    );
    // A default texture switched off writes nothing.
    assert!(effect(json!({"texture_enabled":false,"texture":{}}))
        .get("texture")
        .is_none());
    for (bad, message) in [
        (
            json!({"texture": 5}),
            "effects[0].texture must be an object",
        ),
        (
            json!({"texture": {"depth": "x"}}),
            "effects[0].texture.depth must be a number",
        ),
        (
            json!({"texture": {"invert": 1}}),
            "effects[0].texture.invert must be a boolean",
        ),
        (
            json!({"texture": {"pattern": {"preset": "nope"}}}),
            "effects[0].texture.pattern.preset must be one of",
        ),
        (
            json!({"texture_enabled": "yes"}),
            "effects[0].texture_enabled must be a boolean",
        ),
    ] {
        let mut e = json!({"type":"bevel_emboss"});
        for (k, v) in bad.as_object().unwrap() {
            e[k] = v.clone();
        }
        let err = LayerStyle::from_json(&json!({ "effects": [e] }).to_string()).unwrap_err();
        assert!(err.starts_with(message), "{bad}: {err}");
    }
}

#[test]
fn texture_off_is_byte_identical() {
    let doc = dark();
    let carried = json!({"texture":{"pattern":tile_json(),"depth":3}});
    assert_eq!(hs(&doc, &carried), hs(&doc, &json!({})));
    assert_eq!(
        parse(&bevel(true, json!({})).to_string()).to_json(),
        parse(&bevel(true, json!({"texture_enabled":false})).to_string()).to_json()
    );
}

// ---------------------------------------------------------------- render --

#[test]
fn on_the_flat_interior_the_texture_is_the_oracle() {
    let doc = dark();
    let flat = flat(&doc, RECT, CANVAS.0);
    assert!(flat.len() > 300, "FLAT holds {}", flat.len());
    for (depth, invert) in [(1.0, false), (0.5, false), (2.0, true)] {
        let (h, s) = hs(&doc, &textured(depth, invert, true));
        let gain = G * depth * if invert { -1.0 } else { 1.0 };
        let mut lit = 0;
        for &i in &flat {
            let (x, y) = ((i as u32 % CANVAS.0) as i32, (i as u32 / CANVAS.0) as i32);
            let (wh, ws) = oracle((0, 0), x, y, gain, (0.0, 30.0));
            assert!(
                (i32::from(h[i]) - i32::from(wh)).abs() <= 1
                    && (i32::from(s[i]) - i32::from(ws)).abs() <= 1,
                "depth {depth} invert {invert} at ({x}, {y}): ({}, {}) against ({wh}, {ws})",
                h[i],
                s[i]
            );
            lit += usize::from(wh > 0 || ws > 0);
        }
        assert!(lit > 200, "only {lit} textured pixels");
    }
}

#[test]
fn a_textured_pillow_stops_at_its_reach() {
    // A Pillow Emboss shades a height that is 1 far outside the shape (#578,
    // `style_bevel_smooth`), but its texture is masked by the unreflected
    // height `round(255 · H)`, so nothing lands farther than `smooth_reach`
    // outside the rect (W' + 1 = pair_width(8 / 2) + 1 = 5 at Size 8, Soften
    // 0), and a wider pad (an Inner Shadow of Size 100 beside it, which draws
    // nothing outside) changes no byte outside the shape.
    const REACH: i32 = 4 + 1;
    let doc = dark();
    let mut extra = textured(1.0, false, true);
    extra["style"] = json!("pillow_emboss");
    extra["size"] = json!(8);
    for h in [true, false] {
        let alone = bevel(h, extra.clone());
        let mut beside = alone.clone();
        beside["effects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"inner_shadow","size":100}));
        let (a, b) = (reds(&doc, &alone), reds(&doc, &beside));
        let (mut far, mut lit) = (0, 0);
        for i in 0..a.len() {
            let (x, y) = ((i as u32 % CANVAS.0) as i32, (i as u32 / CANVAS.0) as i32);
            let dx = (RECT.0 - x).max(x - (RECT.0 + RECT.2 as i32 - 1)).max(0);
            let dy = (RECT.1 - y).max(y - (RECT.1 + RECT.3 as i32 - 1)).max(0);
            let d = dx.max(dy);
            if d == 0 {
                continue;
            }
            assert_eq!(
                a[i], b[i],
                "h {h} ({x},{y}): the pad changed an outside byte"
            );
            if d > REACH {
                assert_eq!(a[i], 0, "h {h} ({x},{y}): {d} px out, past the reach");
                far += 1;
            } else if a[i] > 0 {
                lit += 1;
            }
        }
        assert!(
            far > 0 && lit > 0,
            "h {h}: {far} far pixels, {lit} lit near ones"
        );
    }
}

#[test]
fn invert_is_depth_negated() {
    let doc = dark();
    assert_eq!(
        hs(&doc, &textured(1.5, true, true)),
        hs(&doc, &textured(-1.5, false, true))
    );
    assert_ne!(
        hs(&doc, &textured(1.5, true, true)),
        hs(&doc, &textured(1.5, false, true))
    );
}

#[test]
fn a_linked_texture_follows_a_move_and_an_unlinked_one_stays() {
    let doc = dark();
    let flat0 = flat(&doc, RECT, CANVAS.0);
    let shift = (5, 3);
    for link in [true, false] {
        let extra = textured(1.0, false, link);
        let moved = |h: bool| -> Vec<u8> {
            styled(&doc, 1, &bevel(h, extra.clone()).to_string())
                .with_layer_offset(1, RECT.0 + shift.0, RECT.1 + shift.1)
                .unwrap()
                .flattened()
                .unwrap()
                .pixels()
                .map(|p| p.0[0])
                .collect()
        };
        let (h, s) = (moved(true), moved(false));
        let o = if link { shift } else { (0, 0) };
        for &i in &flat0 {
            let (x, y) = (
                (i as u32 % CANVAS.0) as i32 + shift.0,
                (i as u32 / CANVAS.0) as i32 + shift.1,
            );
            let j = (y as u32 * CANVAS.0 + x as u32) as usize;
            let (wh, ws) = oracle(o, x, y, G, (0.0, 30.0));
            assert!(
                (i32::from(h[j]) - i32::from(wh)).abs() <= 1
                    && (i32::from(s[j]) - i32::from(ws)).abs() <= 1,
                "link {link} at ({x}, {y}): ({}, {}) against ({wh}, {ws})",
                h[j],
                s[j]
            );
        }
    }
}

// ---------------------------------------------------------- Photoshop --

fn rows() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/texture/rows.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn photoshops_rows_meet_the_takes_bound() {
    let fixture = rows();
    let bound = &fixture["bound"];
    let (bound_max, bound_mean) = (
        bound["max"].as_f64().unwrap(),
        bound["mean"].as_f64().unwrap(),
    );
    let mut rgba = Vec::new();
    for row in fixture["tile"].as_array().unwrap() {
        for c in row.as_array().unwrap() {
            let c = c.as_array().unwrap();
            rgba.extend(c.iter().map(|v| v.as_u64().unwrap() as u8));
            rgba.push(255);
        }
    }
    let pattern = json!({"name":"T","width":12,"height":8,"rgba":base64(&rgba)});
    let mut judged = 0;
    for (name, row) in fixture["rows"].as_object().unwrap() {
        if !row["score"]["meets"].as_bool().unwrap() {
            continue;
        }
        let scene_name = row["scene"].as_str().unwrap();
        let s = &fixture["scenes"][scene_name];
        let canvas = (
            s["w"].as_u64().unwrap() as u32,
            s["h"].as_u64().unwrap() as u32,
        );
        let r = s["rect"].as_array().unwrap();
        let rect = (
            r[0].as_i64().unwrap() as i32,
            r[1].as_i64().unwrap() as i32,
            r[2].as_u64().unwrap() as u32,
            r[3].as_u64().unwrap() as u32,
        );
        let tx = &row["texture"];
        let extra = json!({"size":row["size"],"depth":row["depth"].as_f64().unwrap() / 100.0,
            "angle":row["angle"],"altitude":row["altitude"],"texture_enabled":true,
            "texture":{"pattern":pattern,"scale":tx["scale"].as_f64().unwrap() / 100.0,
                "depth":tx["depth"].as_f64().unwrap() / 100.0,"invert":tx["invert"],
                "link_with_layer":tx["link"]}});
        let doc = scene(canvas, rect);
        let render = |h: bool| -> Vec<u8> {
            let mut out = styled(&doc, 1, &bevel(h, extra.clone()).to_string());
            if let Some(m) = row["move"].as_array() {
                let (mx, my) = (m[0].as_i64().unwrap() as i32, m[1].as_i64().unwrap() as i32);
                out = out.with_layer_offset(1, rect.0 + mx, rect.1 + my).unwrap();
            }
            out.flattened().unwrap().pixels().map(|p| p.0[0]).collect()
        };
        let (h, s) = (render(true), render(false));
        let (mut worst, mut total, mut n) = (0.0f64, 0.0f64, 0usize);
        for p in row["pixels"].as_array().unwrap() {
            let p: Vec<i64> = p
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap())
                .collect();
            let i = (p[1] as u32 * canvas.0 + p[0] as u32) as usize;
            let e = (i64::from(h[i]) - p[2])
                .abs()
                .max((i64::from(s[i]) - p[3]).abs()) as f64;
            worst = worst.max(e);
            total += e;
            n += 1;
        }
        assert!(n >= 100, "{name}: {n} FLAT pixels");
        let mean = total / n as f64;
        assert!(
            worst <= bound_max && mean <= bound_mean,
            "{name}: max {worst} mean {mean:.3} against {bound_max} / {bound_mean}"
        );
        judged += 1;
    }
    assert!(judged >= 6, "only {judged} rows judged");
}

// ---------------------------------------------------------------- anchors --

#[test]
fn a_linked_texture_anchors_where_the_layer_was_made() {
    // Made at (24, 16), moved by (5, 3) before the style is set.
    let doc = dark().with_layer_offset(1, 29, 19).unwrap();
    assert_eq!(doc.layers[1].reference, (5, 3));
    let linked = styled(
        &doc,
        1,
        &bevel(true, textured(1.0, false, true)).to_string(),
    );
    assert_eq!(effect0(&linked)["texture"]["anchor"], json!([24, 16]));
    let unlinked = styled(
        &doc,
        1,
        &bevel(true, textured(1.0, false, false)).to_string(),
    );
    assert!(effect0(&unlinked)["texture"].get("anchor").is_none());
}

#[test]
fn scale_effects_scales_a_carried_texture() {
    let style = parse(
        &json!({"effects":[{"type":"bevel_emboss","texture":{"scale":0.5,"phase":[3,-2],
            "anchor":[10,5]}}]})
        .to_string(),
    );
    let scaled: Value = serde_json::from_str(&style.scaled(2.0).to_json()).unwrap();
    let t = &scaled["effects"][0]["texture"];
    assert_eq!(t["scale"], json!(1.0));
    assert_eq!(t["phase"], json!([6.0, -4.0]));
    assert_eq!(t["anchor"], json!([20, 10]));
    let plain = parse(&json!({"effects":[{"type":"bevel_emboss","size":5}]}).to_string());
    let scaled: Value = serde_json::from_str(&plain.scaled(2.0).to_json()).unwrap();
    assert!(scaled["effects"][0].get("texture").is_none());
}

/// A 40 x 30 layer made at (-5, -3) whose off-canvas part is transparent.
fn overhanging() -> DrDocument {
    let mut pixels = image::RgbaImage::new(40, 30);
    for y in 8..24 {
        for x in 14..32 {
            pixels.put_pixel(x, y, image::Rgba([0x30, 0xa0, 0x50, 255]));
        }
    }
    let doc = DrDocument::from_pixels(solid(96, 64, [255; 4]))
        .adding_image_layer(0, pixels, "Rect")
        .unwrap();
    let mut doc = doc.with_layer_offset(1, -5, -3).unwrap();
    doc.layers[1].reference = (0, 0);
    doc
}

#[test]
fn a_switched_off_texture_and_a_color_strokes_tile_rebase_on_delete_cropped_pixels() {
    let bevel_off = json!({"effects":[{"type":"bevel_emboss","texture_enabled":false,
        "texture":{"pattern":tile_json(),"anchor":[-5,-3]}}]});
    let color_stroke = json!({"effects":[{"type":"stroke","fill_type":"color",
        "pattern_fill":{"pattern":tile_json(),"anchor":[-5,-3]}}]});
    for (what, style, key) in [
        ("bevel", bevel_off, "texture"),
        ("stroke", color_stroke, "pattern_fill"),
    ] {
        let doc = styled(&overhanging(), 1, &style.to_string());
        assert_eq!(effect0(&doc)[key]["anchor"], json!([-5, -3]), "{what}");
        let before = doc.flattened().unwrap();
        let trimmed = doc
            .delete_cropped_pixels()
            .expect("the layer reaches outside");
        assert_eq!(trimmed.layers[1].offset, (0, 0));
        assert_eq!(effect0(&trimmed)[key]["anchor"], json!([0, 0]), "{what}");
        assert_eq!(trimmed.flattened().unwrap(), before, "{what}: the picture");
    }
}

// ------------------------------------------------------------------ .rz 98 --

fn texture_style(enabled: bool) -> Value {
    json!({"effects":[{"type":"bevel_emboss","texture_enabled":enabled,
        "texture":{"pattern":tile_json(),"depth":-1.5}}]})
}

#[test]
fn the_byte_word_98_comes_first_round_trips_and_refuses_both_ways() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = dark();
    let save = |name: &str, style: &Value| -> (Vec<u8>, DrDocument) {
        let styled_doc = styled(&doc, 1, &style.to_string());
        let path = dir.path().join(name);
        styled_doc.save_native(path.to_str().unwrap()).unwrap();
        (std::fs::read(&path).unwrap(), styled_doc)
    };
    let (plain, _) = save(
        "plain.rz",
        &json!({"effects":[{"type":"bevel_emboss","size":9}]}),
    );
    assert_ne!(
        &plain[8..12],
        &98u32.to_le_bytes(),
        "no 98 without a texture"
    );
    for enabled in [true, false] {
        let name = format!("texture-{enabled}.rz");
        let (bytes, styled_doc) = save(&name, &texture_style(enabled));
        assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
        assert_eq!(&bytes[8..12], &98u32.to_le_bytes(), "FIRST inside 45");
        assert_eq!(&bytes[12..16], &plain[8..12], "then the plain word");
        let path = dir.path().join(&name);
        let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            reopened.layers[1].style.as_deref().map(LayerStyle::to_json),
            styled_doc.layers[1]
                .style
                .as_deref()
                .map(LayerStyle::to_json)
        );
        assert_eq!(
            reopened.flattened().unwrap(),
            styled_doc.flattened().unwrap()
        );
        let mut stripped = bytes.clone();
        stripped.drain(8..12);
        let cut = dir.path().join(format!("cut-{enabled}.rz"));
        std::fs::write(&cut, &stripped).unwrap();
        let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
            panic!("a texture without the word must refuse");
        };
        assert!(
            message.ends_with("native bevel texture requires DRDC 98"),
            "{message}"
        );
    }
    // With noise too, 98 comes before 97.
    let mut both = texture_style(true);
    both["effects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"drop_shadow","noise":0.3}));
    let (bytes, _) = save("both.rz", &both);
    assert_eq!(&bytes[8..12], &98u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &97u32.to_le_bytes(), "98 ahead of 97");
    let mut forged = plain.clone();
    forged.splice(8..8, 98u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = DrDocument::open(path.to_str().unwrap()) else {
        panic!("the word without a texture must refuse");
    };
    assert!(
        message.ends_with("invalid native texture word"),
        "{message}"
    );
}

#[test]
fn a_comp_only_texture_claims_the_word() {
    use doctored_core::doc_comps::{CompCaptures, CompRequest};
    let dir = tempfile::TempDir::new().unwrap();
    let doc = styled(&dark(), 1, &texture_style(true).to_string());
    let save = CompRequest::Save {
        name: "Relief".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = doc.edit_layer_comps(&save).unwrap().unwrap();
    let cleared = try_styled(&saved, 1, None).unwrap().unwrap();
    assert!(cleared.layers[1].style.is_none());
    let path = dir.path().join("comp.rz");
    cleared.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &98u32.to_le_bytes());
    assert!(DrDocument::open(path.to_str().unwrap()).is_ok());
}
