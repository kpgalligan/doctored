//! Gradient glows with Jitter at 8 bits, black-box through
//! `dr_doc_set_layer_style` and the projection: the `fill_type`, `gradient`
//! and `jitter` keys of an Outer and Inner Glow, a Color glow's bytes, the
//! ramp against an oracle written here, Jitter's statistics with the fixed
//! seed, Move and repeat invariance, a restamp, Photoshop's own readings
//! (`fixtures/psd_photoshop/glow_gradient/rows.json`, `glow_gradient_take.py
//! --export-fixture`), the byte `.rz` word 99 and the layer clipboard.
//!
//! The scene is the take's `blur` at half size: a 200 x 150 white canvas
//! under a white 100 x 75 layer made at (50, 38), the style's Fill 0 %, so
//! the glow alone draws over white. Its COLOUR TWIN is the same glow in
//! black, Normal, 100 %: a pixel's `v = 255 − R` is the glow's own plane.
//!
//! The oracle (this file's restatement of the shipped law — take 4's
//! post-take hypothesis, `glow_gradient_take.py` — not the crate's code): `t
//! = 1 − v / 255`, scattered by Jitter `j` to `clamp(t + j (2u − 1))`;
//! within the segment between its bracketing stops `p_i < p_j` the fraction
//! `f` becomes `(f + 3f² − 2f³) / 2`; that `t'` is stored as `round(65535
//! t') / 65535` and the colour `C` is linear between the stops there; the
//! opacity `a` is linear between the stops at the unsmoothed `t`; the
//! coverage is `round(min(255 a, cap))`, `cap = min(255, round(4.125 v₀))`
//! on the Outer Glow (its Below cap, `v₀` the plane before the Range map:
//! the twin at Range 50 %) and 255 on the Inner Glow wherever `v > 0`; the
//! reading is `255 − α / 255 (255 − C)` per channel.

use std::path::PathBuf;

use doctored_core::doc::DrDocument;
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use crate::common::{parse, rect_layer_doc, styled, take_err_string, try_styled};

const CANVAS: (u32, u32) = (200, 150);
const RECT: (i32, i32, u32, u32) = (50, 38, 100, 75);
/// `style_contour`'s outer-glow Below cap (half the drop shadow's 8.25).
const K: f64 = 4.125;

// ---------------------------------------------------------------- helpers --

fn scene() -> DrDocument {
    rect_layer_doc(CANVAS, RECT, [255, 255, 255, 255])
}

/// A glow of `kind` (black, Normal, 100 %, size 40) with `extra` merged.
fn glow(kind: &str, extra: Value) -> Value {
    let mut e = json!({"type":kind,"blend":"normal","opacity":1,"color":"#000000","size":40});
    for (k, v) in extra.as_object().unwrap() {
        e[k] = v.clone();
    }
    e
}

/// The RGB of `doc` with `effect` on layer 1 at Fill 0.
fn render(doc: &DrDocument, effect: &Value) -> Vec<[u8; 3]> {
    let style = json!({"fill_opacity":0,"effects":[effect]});
    styled(doc, 1, &style.to_string())
        .flattened()
        .unwrap()
        .pixels()
        .map(|p| [p.0[0], p.0[1], p.0[2]])
        .collect()
}

/// The colour twin's plane `v = 255 − R` (module doc).
fn twin(doc: &DrDocument, effect: &Value) -> Vec<u8> {
    let mut e = effect.clone();
    let o = e.as_object_mut().unwrap();
    for key in ["fill_type", "gradient", "jitter"] {
        o.remove(key);
    }
    o.insert("color".into(), json!("#000000"));
    render(doc, &e).iter().map(|p| 255 - p[0]).collect()
}

struct Stop {
    position: f64,
    rgb: [f64; 3],
    opacity: f64,
}

fn hex(text: &str) -> [f64; 3] {
    let v = u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap();
    [(v >> 16) & 255, (v >> 8) & 255, v & 255].map(f64::from)
}

fn stops(gradient: &Value) -> Vec<Stop> {
    gradient["stops"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| Stop {
            position: s["position"].as_f64().unwrap(),
            rgb: hex(s["color"].as_str().unwrap()),
            opacity: s["opacity"].as_f64().unwrap_or(1.0),
        })
        .collect()
}

/// The bracketing stops of `t` and its fraction, `None` at or past an end.
fn bracket(stops: &[Stop], t: f64) -> Option<(usize, f64)> {
    if t <= stops[0].position || t >= stops[stops.len() - 1].position {
        return None;
    }
    (0..stops.len() - 1).find_map(|i| {
        let (a, b) = (stops[i].position, stops[i + 1].position);
        (t >= a && t < b).then(|| (i, if b > a { (t - a) / (b - a) } else { 1.0 }))
    })
}

fn smooth(stops: &[Stop], t: f64) -> f64 {
    match bracket(stops, t) {
        Some((i, f)) => {
            let (a, b) = (stops[i].position, stops[i + 1].position);
            a + (b - a) * (f + 3.0 * f * f - 2.0 * f * f * f) / 2.0
        }
        None => t,
    }
}

fn ramp(stops: &[Stop], t: f64) -> ([f64; 3], f64) {
    match bracket(stops, t) {
        Some((i, f)) => {
            let (a, b) = (&stops[i], &stops[i + 1]);
            (
                [0, 1, 2].map(|c| a.rgb[c] + (b.rgb[c] - a.rgb[c]) * f),
                a.opacity + (b.opacity - a.opacity) * f,
            )
        }
        None => {
            let s = if t <= stops[0].position {
                &stops[0]
            } else {
                &stops[stops.len() - 1]
            };
            (s.rgb, s.opacity)
        }
    }
}

/// The oracle's reading at plane value `v` (`v0` before the Range map) with
/// the unsmoothed `t`.
fn predict(stops: &[Stop], outer: bool, v0: u8, t: f64) -> [f64; 3] {
    let t = t.clamp(0.0, 1.0);
    let (_, a) = ramp(stops, t);
    let ts = (smooth(stops, t) * 65535.0).round() / 65535.0;
    let (c, _) = ramp(stops, ts);
    let cap = if outer {
        (K * f64::from(v0)).round().min(255.0)
    } else {
        255.0
    };
    let alpha = (255.0 * a).min(cap).round();
    c.map(|c| 255.0 - alpha / 255.0 * (255.0 - c))
}

fn three() -> Value {
    json!({"stops":[{"position":0,"color":"#000000","opacity":1},
        {"position":0.5,"color":"#ff0000","opacity":0.6},
        {"position":1,"color":"#ffffff","opacity":1}]})
}

fn black_white() -> Value {
    json!({"stops":[{"position":0,"color":"#000000"},{"position":1,"color":"#ffffff"}]})
}

fn effect0(doc: &DrDocument) -> Value {
    let style: Value =
        serde_json::from_str(&doc.layers[1].style.as_deref().unwrap().to_json()).unwrap();
    style["effects"][0].clone()
}

// ------------------------------------------------------------------- JSON --

fn written(effect: Value) -> Value {
    let style = parse(&json!({ "effects": [effect] }).to_string());
    let v: Value = serde_json::from_str(&style.to_json()).unwrap();
    v["effects"][0].clone()
}

#[test]
fn glow_fill_json_parses_resets_clamps_and_writes_only_when_carried() {
    for kind in ["outer_glow", "inner_glow"] {
        let plain = written(json!({ "type": kind }));
        for key in ["fill_type", "gradient", "jitter"] {
            assert!(plain.get(key).is_none(), "{kind}: {key} in {plain}");
        }
        assert_eq!(
            written(json!({"type":kind,"fill_type":"color"})),
            plain,
            "{kind}: an explicit color is the default"
        );
        let g = written(json!({"type":kind,"fill_type":"gradient","jitter":2,
            "gradient":{"stops":[{"position":0.2,"color":"#FF0000"},
                {"position":0.9,"color":"#0000ff","opacity":0.25}],
                "style":"radial","angle":30,"reverse":true,"scale":1.4,
                "align_with_layer":false}}));
        assert_eq!(g["fill_type"], json!("gradient"));
        assert_eq!(g["jitter"], json!(1.0), "jitter clamps to 1");
        assert_eq!(
            g["gradient"],
            json!({"stops":[{"position":0.2,"color":"#ff0000","opacity":1.0},
                {"position":0.9,"color":"#0000ff","opacity":0.25}],
                "style":"linear","angle":90.0,"scale":1.0,"reverse":false,
                "align_with_layer":true}),
            "{kind}: every key but the stops is reset"
        );
        // A Color glow keeps a stored gradient and a jitter.
        let kept = written(json!({"type":kind,"jitter":0.123456,
            "gradient":{"stops":[{"position":0,"color":"#102030"},{"position":1}]}}));
        assert!(kept.get("fill_type").is_none());
        assert_eq!(kept["jitter"], json!(0.1235));
        assert_eq!(kept["gradient"]["stops"][0]["color"], json!("#102030"));
        // The default stops on a Color glow write nothing.
        assert!(written(json!({"type":kind,"gradient":black_white()}))
            .get("gradient")
            .is_none());
        for (bad, message) in [
            (
                json!({"fill_type":"pattern"}),
                "effects[0].fill_type must be one of color, gradient",
            ),
            (json!({"jitter":"x"}), "effects[0].jitter must be a number"),
            (
                json!({"gradient":5}),
                "effects[0].gradient must be an object",
            ),
        ] {
            let mut e = json!({ "type": kind });
            for (k, v) in bad.as_object().unwrap() {
                e[k] = v.clone();
            }
            let text = json!({ "effects": [e] }).to_string();
            let err = LayerStyle::from_json(&text).unwrap_err();
            assert!(err.starts_with(message), "{bad}: {err}");
        }
        // Lenient: an unknown fill reads as Color.
        let lenient = LayerStyle::from_json_lenient(
            &json!({"effects":[{"type":kind,"fill_type":"pattern"}]}).to_string(),
        )
        .unwrap();
        assert_eq!(
            lenient.to_json(),
            parse(&json!({ "effects": [{ "type": kind }] }).to_string()).to_json()
        );
    }
}

#[test]
fn a_color_glow_is_byte_identical_whatever_it_stores() {
    let doc = scene();
    for kind in ["outer_glow", "inner_glow"] {
        let plain = glow(kind, json!({"color":"#3060c0"}));
        let stored = glow(
            kind,
            json!({"color":"#3060c0","fill_type":"color","gradient":three(),"jitter":0.7}),
        );
        assert_eq!(render(&doc, &stored), render(&doc, &plain), "{kind}");
    }
}

// ---------------------------------------------------------------- render --

#[test]
fn the_ramp_is_the_oracle_at_every_size_range_and_source() {
    let doc = scene();
    let gradient = three();
    let s = stops(&gradient);
    let cases = [
        ("outer_glow", json!({"size":20})),
        ("outer_glow", json!({"size":40})),
        ("outer_glow", json!({"size":40,"range":0.25})),
        ("outer_glow", json!({"size":40,"range":1})),
        ("outer_glow", json!({"size":20,"range":0.25})),
        ("inner_glow", json!({"size":20})),
        ("inner_glow", json!({"size":40})),
        ("inner_glow", json!({"size":40,"range":0.25})),
        ("inner_glow", json!({"size":40,"range":1})),
        ("inner_glow", json!({"size":20,"source":"center"})),
        ("inner_glow", json!({"size":40,"source":"center"})),
    ];
    for (kind, geometry) in cases {
        let base = glow(kind, geometry.clone());
        let v = twin(&doc, &base);
        // The plane before the Range map: the same glow at Range 50 %.
        let mut unmapped = base.clone();
        unmapped["range"] = json!(0.5);
        let v0 = twin(&doc, &unmapped);
        let mut e = base.clone();
        e["fill_type"] = json!("gradient");
        e["gradient"] = gradient.clone();
        let got = render(&doc, &e);
        let mut lit = 0;
        for (i, (&v, g)) in v.iter().zip(&got).enumerate() {
            if v == 0 {
                assert_eq!(
                    *g, [255; 3],
                    "{kind} {geometry}: pixel {i} outside the glow"
                );
                continue;
            }
            let want = predict(&s, kind == "outer_glow", v0[i], 1.0 - f64::from(v) / 255.0);
            for c in 0..3 {
                assert!(
                    (f64::from(g[c]) - want[c].round()).abs() <= 1.0,
                    "{kind} {geometry}: pixel {i} (v {v}) channel {c}: {g:?} against {want:?}"
                );
            }
            lit += 1;
        }
        assert!(lit > 2000, "{kind} {geometry}: only {lit} pixels lit");
    }
}

#[test]
fn stop_opacity_is_the_coverage_held_to_the_outer_glows_cap() {
    // A fade of one colour, 100 % to 0 %: the coverage is the stops' linear
    // opacity (held to the Outer Glow's Below cap where that is smaller),
    // never the plane times it.
    let doc = scene();
    let fade = json!({"stops":[{"position":0,"color":"#204080","opacity":1},
        {"position":1,"color":"#204080","opacity":0}]});
    let s = stops(&fade);
    for kind in ["outer_glow", "inner_glow"] {
        let colour = glow(kind, json!({"color":"#204080"}));
        let mut gradient = colour.clone();
        gradient["fill_type"] = json!("gradient");
        gradient["gradient"] = fade.clone();
        let got = render(&doc, &gradient);
        let v = twin(&doc, &colour);
        let mut lit = 0;
        for (i, (q, &v)) in got.iter().zip(&v).enumerate() {
            let want = predict(&s, kind == "outer_glow", v, 1.0 - f64::from(v) / 255.0);
            for c in 0..3 {
                assert!(
                    (f64::from(q[c]) - want[c].round()).abs() <= 1.0,
                    "{kind}: pixel {i} channel {c}: {q:?} against {want:?}"
                );
            }
            lit += usize::from(*q != [255; 3]);
        }
        assert!(lit > 2000, "{kind}: only {lit} pixels lit");
    }
}

/// The per-bin judge (module doc of `glow_gradient_take.py`, BOUNDS): the
/// row's codes `255 − R` binned by twin `v // 16`, `v >= 32`, against the
/// mixture of the oracle over 512 midpoints of `u`.
fn jitter_bins(got: &[[u8; 3]], v: &[u8], s: &[Stop], outer: bool, j: f64) -> usize {
    let mut judged = 0;
    for bin in 2..16u8 {
        let members: Vec<usize> = (0..v.len()).filter(|&i| v[i] / 16 == bin).collect();
        if members.len() < 200 {
            continue;
        }
        let n = members.len() as f64;
        let codes: Vec<f64> = members
            .iter()
            .map(|&i| 255.0 - f64::from(got[i][0]))
            .collect();
        let mean = codes.iter().sum::<f64>() / n;
        let sd = (codes.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / n).sqrt();
        let (mut s1, mut s2) = (0.0, 0.0);
        for &i in &members {
            let t = 1.0 - f64::from(v[i]) / 255.0;
            for k in 0..512 {
                let u = (f64::from(k) + 0.5) / 512.0;
                let r = predict(s, outer, v[i], t + j * (2.0 * u - 1.0))[0].round();
                let c = 255.0 - r;
                s1 += c / 512.0;
                s2 += c * c / 512.0;
            }
        }
        let want_mean = s1 / n;
        let want_sd = (s2 / n - want_mean * want_mean).max(0.0).sqrt();
        let allow_mean = 2.0 + 3.0 * sd / n.sqrt();
        let allow_sd = 2.0 + 3.0 * sd / (2.0 * n).sqrt();
        assert!(
            (mean - want_mean).abs() <= allow_mean && (sd - want_sd).abs() <= allow_sd,
            "bin {bin}: mean {mean:.2} sd {sd:.2} against {want_mean:.2} / {want_sd:.2} \
             (allow {allow_mean:.2} / {allow_sd:.2}, n {n})"
        );
        judged += 1;
    }
    judged
}

#[test]
fn jitter_follows_the_spread_law_per_bin() {
    let doc = scene();
    let s = stops(&black_white());
    for (kind, j) in [
        ("outer_glow", 0.5),
        ("outer_glow", 1.0),
        ("inner_glow", 0.5),
    ] {
        let base = glow(kind, json!({}));
        let v = twin(&doc, &base);
        let mut e = base.clone();
        e["fill_type"] = json!("gradient");
        e["gradient"] = black_white();
        e["jitter"] = json!(j);
        let got = render(&doc, &e);
        let judged = jitter_bins(&got, &v, &s, kind == "outer_glow", j);
        assert!(judged >= 3, "{kind} {j}: only {judged} bins judged");
        // Jitter scatters: the row is not the unjittered ramp.
        e["jitter"] = json!(0);
        assert_ne!(render(&doc, &e), got, "{kind} {j}");
    }
}

#[test]
fn jitter_repeats_exactly_and_moves_with_the_layer() {
    let doc = scene();
    let e = glow(
        "outer_glow",
        json!({"size":20,"fill_type":"gradient","gradient":three(),"jitter":0.5}),
    );
    let style = json!({"fill_opacity":0,"effects":[e]}).to_string();
    let a = render(&doc, &e);
    assert_eq!(render(&scene(), &e), a, "a fresh render repeats exactly");
    let (dx, dy) = (13, 7);
    let moved = styled(&doc, 1, &style)
        .with_layer_offset(1, RECT.0 + dx, RECT.1 + dy)
        .unwrap()
        .flattened()
        .unwrap();
    let w = CANVAS.0 as i32;
    let mut compared = 0;
    for y in 0..CANVAS.1 as i32 - dy {
        for x in 0..w - dx {
            let p = moved.get_pixel((x + dx) as u32, (y + dy) as u32).0;
            let q = a[(y * w + x) as usize];
            assert_eq!([p[0], p[1], p[2]], q, "({x}, {y})");
            compared += usize::from(q != [255; 3]);
        }
    }
    assert!(compared > 2000, "only {compared} glow pixels compared");
}

#[test]
fn a_colour_edit_restamps_the_cached_planes_as_a_fresh_render() {
    let doc = scene();
    for kind in ["outer_glow", "inner_glow"] {
        let first = glow(
            kind,
            json!({"fill_type":"gradient","gradient":three(),"jitter":0.25}),
        );
        let mut second = first.clone();
        second["gradient"]["stops"][1]["color"] = json!("#00ff80");
        second["blend"] = json!("multiply");
        second["opacity"] = json!(0.7);
        let warm = styled(
            &doc,
            1,
            &json!({"fill_opacity":0,"effects":[first]}).to_string(),
        );
        warm.flattened().unwrap();
        let text = json!({"fill_opacity":0,"effects":[second]}).to_string();
        let restamped = styled(&warm, 1, &text).flattened().unwrap();
        let fresh = styled(&doc, 1, &text).flattened().unwrap();
        assert_eq!(restamped, fresh, "{kind}");
        // A switch to Color re-renders and is the colour glow.
        let mut colour = second.clone();
        colour["fill_type"] = json!("color");
        let text = json!({"fill_opacity":0,"effects":[colour]}).to_string();
        assert_eq!(
            styled(&warm, 1, &text).flattened().unwrap(),
            styled(&doc, 1, &text).flattened().unwrap(),
            "{kind}: to Color"
        );
    }
}

// ---------------------------------------------------------- Photoshop --

fn rows() -> Option<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/glow_gradient/rows.json");
    std::fs::read_to_string(path)
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap())
}

/// The take's scene (full size) with `effect`'s geometry.
fn take_scene(fixture: &Value) -> DrDocument {
    let s = &fixture["scene"];
    let r = s["rect"].as_array().unwrap();
    rect_layer_doc(
        (
            s["w"].as_u64().unwrap() as u32,
            s["h"].as_u64().unwrap() as u32,
        ),
        (
            r[0].as_i64().unwrap() as i32,
            r[1].as_i64().unwrap() as i32,
            r[2].as_u64().unwrap() as u32,
            r[3].as_u64().unwrap() as u32,
        ),
        [255; 4],
    )
}

fn take_effect(fx: &Value) -> Value {
    let mut e = json!({"type":fx["type"],"blend":"normal","opacity":1,"color":"#000000",
        "size":fx["size"],"range":fx["range"],"fill_type":"gradient",
        "gradient":{"stops":fx["gradient"]},"jitter":fx["jitter"]});
    if fx["type"] == json!("inner_glow") {
        e["source"] = fx["source"].clone();
    }
    e
}

#[test]
fn photoshops_rows_meet_the_takes_bound_through_the_glow_plane() {
    let Some(fixture) = rows() else {
        panic!("tests/fixtures/psd_photoshop/glow_gradient/rows.json is committed");
    };
    let doc = take_scene(&fixture);
    let bound = &fixture["bound"];
    let (bound_max, bound_mean) = (
        bound["max"].as_f64().unwrap(),
        bound["mean"].as_f64().unwrap(),
    );
    let mut judged = 0;
    for (name, row) in fixture["placement"].as_object().unwrap() {
        let e = take_effect(&row["effect"]);
        let v = twin(&doc, &e);
        let got = render(&doc, &e);
        // Photoshop's mean reading per twin code (the transfer from glow
        // plane to colour): Doctored's reading at its own twin code is
        // held to it, so the glow's own profile residual does not enter.
        let mut table = [None; 256];
        for entry in row["table"].as_array().unwrap() {
            let e: Vec<f64> = entry
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect();
            table[e[0] as usize] = Some([e[2], e[3], e[4]]);
        }
        let (mut worst, mut total, mut n) = (0.0f64, 0.0f64, 0usize);
        for (&v, g) in v.iter().zip(&got) {
            let Some(want) = (v > 0).then(|| table[usize::from(v)]).flatten() else {
                continue;
            };
            let e = (0..3)
                .map(|c| (f64::from(g[c]) - want[c]).abs())
                .fold(0.0, f64::max);
            worst = worst.max(e);
            total += e;
            n += 1;
        }
        assert!(n > 1000, "{name}: {n} pixels");
        let mean = total / n as f64;
        assert!(
            worst <= bound_max && mean <= bound_mean,
            "{name}: max {worst} mean {mean:.3} against {bound_max} / {bound_mean}"
        );
        judged += 1;
    }
    for (name, row) in fixture["jitter"].as_object().unwrap() {
        let e = take_effect(&row["effect"]);
        let v = twin(&doc, &e);
        let got = render(&doc, &e);
        for bin in row["bins"].as_array().unwrap() {
            let lo = bin["bin"].as_u64().unwrap() as u8;
            let members: Vec<usize> = (0..v.len())
                .filter(|&i| v[i] >= lo.max(32) && u32::from(v[i]) < u32::from(lo) + 16)
                .collect();
            let n = members.len() as f64;
            if n < 100.0 {
                continue;
            }
            for (ch, stat) in bin["stats"].as_array().unwrap().iter().enumerate() {
                let codes: Vec<f64> = members
                    .iter()
                    .map(|&i| 255.0 - f64::from(got[i][ch]))
                    .collect();
                let mean = codes.iter().sum::<f64>() / n;
                let sd = (codes.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / n).sqrt();
                let (pm, psd) = (stat[0].as_f64().unwrap(), stat[1].as_f64().unwrap());
                let ps_n = bin["n"].as_f64().unwrap();
                let se = 3.0 * psd / ps_n.sqrt() + 3.0 * sd / n.sqrt();
                assert!(
                    (mean - pm).abs() <= 2.0 + se && (sd - psd).abs() <= 2.0 + se,
                    "{name} bin {lo} channel {ch}: {mean:.2} / {sd:.2} against Photoshop's \
                     {pm:.2} / {psd:.2} (n {n} / {ps_n})"
                );
            }
        }
        judged += 1;
    }
    assert!(judged >= 4, "only {judged} rows judged");
}

// ------------------------------------------------------------------ .rz 99 --

fn gradient_style(fill: &str, jitter: f64) -> Value {
    json!({"effects":[{"type":"outer_glow","fill_type":fill,"gradient":three(),
        "jitter":jitter,"size":12}]})
}

#[test]
fn the_byte_word_99_comes_first_round_trips_and_refuses_both_ways() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = scene();
    let save = |name: &str, style: &Value| -> (Vec<u8>, DrDocument) {
        let styled_doc = styled(&doc, 1, &style.to_string());
        let path = dir.path().join(name);
        styled_doc.save_native(path.to_str().unwrap()).unwrap();
        (std::fs::read(&path).unwrap(), styled_doc)
    };
    let (plain, _) = save(
        "plain.rz",
        &json!({"effects":[{"type":"outer_glow","size":9},{"type":"inner_glow","size":4}]}),
    );
    assert_ne!(
        &plain[8..12],
        &99u32.to_le_bytes(),
        "no 99 without a gradient glow"
    );
    for (i, (fill, jitter)) in [
        ("gradient", 0.0),
        ("gradient", 0.4),
        ("color", 0.0),
        ("color", 0.3),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("glow-{i}.rz");
        let (bytes, styled_doc) = save(&name, &gradient_style(fill, jitter));
        assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
        assert_eq!(&bytes[8..12], &99u32.to_le_bytes(), "FIRST inside 45");
        let path = dir.path().join(&name);
        let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
        assert_eq!(effect0(&reopened), effect0(&styled_doc), "{fill} {jitter}");
        assert_eq!(
            reopened.flattened().unwrap(),
            styled_doc.flattened().unwrap()
        );
        let mut stripped = bytes.clone();
        stripped.drain(8..12);
        let cut = dir.path().join(format!("cut-{i}.rz"));
        std::fs::write(&cut, &stripped).unwrap();
        let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
            panic!("a gradient glow without the word must refuse");
        };
        assert!(
            message.ends_with("native gradient glow requires DRDC 99"),
            "{message}"
        );
    }
    // With a texture too, 99 comes before 98.
    let mut both = gradient_style("gradient", 0.5);
    both["effects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"bevel_emboss","texture_enabled":true}));
    let (bytes, _) = save("both.rz", &both);
    assert_eq!(&bytes[8..12], &99u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &98u32.to_le_bytes(), "99 ahead of 98");
    let mut forged = plain.clone();
    forged.splice(8..8, 99u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = DrDocument::open(path.to_str().unwrap()) else {
        panic!("the word without a gradient glow must refuse");
    };
    assert!(
        message.ends_with("invalid native glow gradient word"),
        "{message}"
    );
}

#[test]
fn a_comp_only_gradient_glow_claims_the_word() {
    use doctored_core::doc_comps::{CompCaptures, CompRequest};
    let dir = tempfile::TempDir::new().unwrap();
    let doc = styled(&scene(), 1, &gradient_style("gradient", 0.2).to_string());
    let save = CompRequest::Save {
        name: "Glow".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = doc.edit_layer_comps(&save).unwrap().unwrap();
    let cleared = try_styled(&saved, 1, None).unwrap().unwrap();
    assert!(cleared.layers[1].style.is_none());
    let path = dir.path().join("comp.rz");
    cleared.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &99u32.to_le_bytes());
    assert!(DrDocument::open(path.to_str().unwrap()).is_ok());
}

// --------------------------------------------------------- the clipboard --

#[test]
fn the_layer_clipboard_carries_a_gradient_glow() {
    use doctored_core::ffi_layer_clipboard::dr_doc_paste_layers;
    let source = styled(
        &scene(),
        1,
        &json!({"fill_opacity":0,"effects":[glow("inner_glow",
            json!({"fill_type":"gradient","gradient":three(),"jitter":0.6,"source":"center"}))]})
        .to_string(),
    );
    let target = doctored_core::doc::DrDocument::from_pixels(crate::common::solid(
        CANVAS.0, CANVAS.1, [255; 4],
    ));
    let mut err = std::ptr::null_mut();
    let out = unsafe { dr_doc_paste_layers(&target, &source, 0, 1, 0, 0, false, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let pasted = unsafe { Box::from_raw(out) };
    let top = pasted.layers.len() - 1;
    assert_eq!(
        pasted.layers[top].style.as_deref().map(LayerStyle::to_json),
        source.layers[1].style.as_deref().map(LayerStyle::to_json)
    );
}
