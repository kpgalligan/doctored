//! Layer style CONTOURS: the contour keys of the style
//! JSON, and every contoured effect against an oracle written here. Each
//! effect is drawn black, Normal, 100 % over the white canvas of
//! `common::rect_layer_doc`, the layer itself at `fill_opacity` 0, so a
//! pixel's coverage is `a = 255 − R`. The oracles:
//!
//! - a contoured effect equals the test's own table `L` applied to its
//!   Linear twin's coverage, `L[a]`; on a Below effect `L` is capped at
//!   `k · R(a)` (the take's REFIT: `k` 8.25 on the drop shadow, half that
//!   on the outer glow);
//! - a custom curve's `L` is the natural cubic spline written below (run by
//!   run between corners, flat past its end points);
//! - each preset's table is the take's measured table in
//!   `fixtures/psd_photoshop/contour/tables.json`;
//! - Photoshop's own readings (`rows.json`: per row, its mean reading per
//!   twin code, from `contour_take.py`'s TIFFs) are held to the take's
//!   bound, max 6 and mean 1.5 codes.

use std::collections::BTreeMap;
use std::path::PathBuf;

use doctored_core::doc::DrDocument;
use doctored_core::style::*;
use serde_json::{json, Value};

use crate::common::{rect_layer_doc, styled};

// ------------------------------------------------- the take's winners --

/// The gloss support (`contour_take.py`'s EXTENT winner): a pixel's gloss
/// is mapped only where one of its two pre-gloss bytes reaches this.
const GLOSS_SUPPORT: u8 = 8;
/// The Below caps (the take's REFIT): `min(C(R(a)), k R(a))` on the drop
/// shadow and the outer glow.
const CAP_DS: f64 = 8.25;
const CAP_OG: f64 = 8.25 / 2.0;
/// The take's bound, in 8-bit codes.
const BOUND_MAX: f64 = 6.0;
const BOUND_MEAN: f64 = 1.5;

/// The take's RANGE winner on `x` in 0..1 at range `r` (the glows).
fn range_x(x: f64, r: f64) -> f64 {
    (x / (2.0 * r)).min(1.0)
}

// ---------------------------------------------------------- the oracle --

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/contour")
        .join(name)
}

fn tables() -> BTreeMap<String, Vec<f64>> {
    let text = std::fs::read_to_string(fixture("tables.json")).expect("tables.json");
    let v: Value = serde_json::from_str(&text).unwrap();
    v["tables"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, t)| {
            (
                k.clone(),
                t.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect(),
            )
        })
        .collect()
}

/// A natural cubic spline through `points`, written here from its
/// definition (second derivative 0 at both ends; the tridiagonal system by
/// Gaussian elimination), flat past the end points, clamped to 0..255.
fn natural_spline(points: &[(f64, f64)], x: f64) -> f64 {
    let n = points.len();
    if x <= points[0].0 {
        return points[0].1;
    }
    if x >= points[n - 1].0 {
        return points[n - 1].1;
    }
    let mut m = vec![0.0; n];
    if n > 2 {
        // Rows 1..n-1 of [h_{i-1}, 2(h_{i-1} + h_i), h_i] m = 6 (d_i − d_{i−1}).
        let k = n - 2;
        let mut a = vec![vec![0.0; k + 1]; k];
        for row in 0..k {
            let i = row + 1;
            let h0 = points[i].0 - points[i - 1].0;
            let h1 = points[i + 1].0 - points[i].0;
            let d0 = (points[i].1 - points[i - 1].1) / h0;
            let d1 = (points[i + 1].1 - points[i].1) / h1;
            if row > 0 {
                a[row][row - 1] = h0;
            }
            a[row][row] = 2.0 * (h0 + h1);
            if row + 1 < k {
                a[row][row + 1] = h1;
            }
            a[row][k] = 6.0 * (d1 - d0);
        }
        for col in 0..k {
            for row in col + 1..k {
                let f = a[row][col] / a[col][col];
                let pivot = a[col].clone();
                for (cell, p) in a[row].iter_mut().zip(&pivot).skip(col) {
                    *cell -= f * p;
                }
            }
        }
        for row in (0..k).rev() {
            let mut v = a[row][k];
            for c in row + 1..k {
                v -= a[row][c] * m[c + 1];
            }
            m[row + 1] = v / a[row][row];
        }
    }
    let i = (0..n - 1).find(|&i| x <= points[i + 1].0).unwrap();
    let (x0, y0, x1, y1) = (points[i].0, points[i].1, points[i + 1].0, points[i + 1].1);
    let h = x1 - x0;
    let (t, u) = ((x1 - x) / h, (x - x0) / h);
    (t * y0 + u * y1 + ((t * t * t - t) * m[i] + (u * u * u - u) * m[i + 1]) * h * h / 6.0)
        .clamp(0.0, 255.0)
}

/// A custom curve `[x, y, corner]`: one spline per run between corners.
fn custom(points: &[(f64, f64, bool)], x: f64) -> f64 {
    let mut runs: Vec<Vec<(f64, f64)>> = vec![vec![(points[0].0, points[0].1)]];
    for (i, p) in points.iter().enumerate().skip(1) {
        runs.last_mut().unwrap().push((p.0, p.1));
        if p.2 && i + 1 < points.len() {
            runs.push(vec![(p.0, p.1)]);
        }
    }
    let run = runs
        .iter()
        .find(|r| x <= r.last().unwrap().0)
        .unwrap_or(runs.last().unwrap());
    natural_spline(run, x)
}

/// `values` read piecewise linearly at a fractional code.
fn read(values: &[f64], at: f64) -> f64 {
    let at = at.clamp(0.0, 255.0);
    let i = (at.floor() as usize).min(254);
    values[i] + (at - i as f64) * (values[i + 1] - values[i])
}

/// The oracle table `L[c] = round(min(T(255 R(c / 255)), cap 255 R(c / 255)))`
/// (module doc). Anti-aliased draws nothing (the take's AA winner, NONE),
/// so it has no parameter here.
fn lut(values: &[f64], range: Option<f64>, cap: Option<f64>) -> Vec<u8> {
    (0..256)
        .map(|c| {
            let x = c as f64 / 255.0;
            let x = range.map_or(x, |r| range_x(x, r));
            let mut v = read(values, 255.0 * x);
            if let Some(k) = cap {
                v = v.min(k * 255.0 * x);
            }
            v.round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

fn custom_values(points: &[(f64, f64, bool)]) -> Vec<f64> {
    (0..256).map(|c| custom(points, c as f64)).collect()
}

const CU1: [(f64, f64, bool); 4] = [
    (0.0, 0.0, false),
    (64.0, 170.0, false),
    (150.0, 60.0, false),
    (255.0, 255.0, false),
];
const CU2: [(f64, f64, bool); 4] = [
    (0.0, 40.0, false),
    (96.0, 230.0, true),
    (200.0, 90.0, false),
    (255.0, 200.0, false),
];

fn points_json(points: &[(f64, f64, bool)]) -> Value {
    Value::Array(
        points
            .iter()
            .map(|p| {
                if p.2 {
                    json!({"x": p.0, "y": p.1, "corner": true})
                } else {
                    json!({"x": p.0, "y": p.1})
                }
            })
            .collect(),
    )
}

// ---------------------------------------------------------- the scenes --

const CANVAS: (u32, u32) = (240, 160);
const RECT: (i32, i32, u32, u32) = (60, 40, 120, 80);

fn coverage(doc: &DrDocument, effect: Value) -> Vec<u8> {
    let style = json!({"fill_opacity": 0, "effects": [effect]});
    let out = styled(doc, 1, &style.to_string());
    out.flattened()
        .unwrap()
        .pixels()
        .map(|p| 255 - p.0[0])
        .collect()
}

fn scene() -> DrDocument {
    rect_layer_doc(CANVAS, RECT, [0, 0, 0, 255])
}

fn inside(i: usize) -> bool {
    let (x, y) = ((i as u32 % CANVAS.0) as i32, (i as u32 / CANVAS.0) as i32);
    x >= RECT.0 && y >= RECT.1 && x < RECT.0 + RECT.2 as i32 && y < RECT.1 + RECT.3 as i32
}

fn with(mut effect: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        effect[k] = v.clone();
    }
    effect
}

fn drop_shadow(size: f64) -> Value {
    json!({"type":"drop_shadow","blend":"normal","color":"#000000","opacity":1,
        "distance":0,"spread":0,"size":size,"layer_knocks_out":false,"use_global_light":false})
}

fn outer_glow(size: f64) -> Value {
    json!({"type":"outer_glow","blend":"normal","color":"#000000","opacity":1,"spread":0,
        "size":size})
}

fn inner_glow(size: f64) -> Value {
    json!({"type":"inner_glow","blend":"normal","color":"#000000","opacity":1,"choke":0,
        "size":size})
}

fn inner_shadow(size: f64) -> Value {
    json!({"type":"inner_shadow","blend":"normal","color":"#000000","opacity":1,"choke":0,
        "distance":0,"size":size,"use_global_light":false})
}

fn satin() -> Value {
    json!({"type":"satin","blend":"normal","color":"#000000","opacity":1,"angle":19,
        "distance":11,"size":14,"invert":true})
}

/// Every pixel of `got` against `table[twin]`, restricted to `keep`; exact.
fn assert_mapped(twin: &[u8], got: &[u8], table: &[u8], keep: impl Fn(usize) -> bool, what: &str) {
    let mut n = 0;
    for (i, (&a, &g)) in twin.iter().zip(got).enumerate() {
        if !keep(i) {
            continue;
        }
        let want = table[usize::from(a)];
        assert_eq!(g, want, "{what}: pixel {i} twin {a}");
        n += 1;
    }
    assert!(n > 100, "{what}: only {n} pixels judged");
}

// ------------------------------------------------------------ the JSON --

#[test]
fn default_effects_write_no_contour_key() {
    let style = LayerStyle::from_json(
        r#"{"effects":[{"type":"drop_shadow"},{"type":"inner_shadow"},{"type":"outer_glow"},
        {"type":"inner_glow"},{"type":"satin"},{"type":"bevel_emboss"}]}"#,
    )
    .unwrap();
    let text = style.to_json();
    for key in [
        "contour",
        "anti_aliased",
        "\"range\"",
        "gloss_contour",
        "gloss_anti_aliased",
        "contour_enabled",
        "contour_anti_aliased",
        "contour_range",
    ] {
        assert!(!text.contains(key), "{key} written at its default: {text}");
    }
}

#[test]
fn every_contour_key_round_trips() {
    let json = json!({"effects":[
        {"type":"drop_shadow","contour":{"preset":"cone"},"anti_aliased":true},
        {"type":"inner_shadow","contour":{"preset":"sawtooth_1"}},
        {"type":"outer_glow","contour":{"preset":"ring"},"range":0.25},
        {"type":"inner_glow","contour":{"points":points_json(&CU2)},"range":0.75,
            "anti_aliased":true},
        {"type":"satin","contour":{"preset":"gaussian"}},
        {"type":"bevel_emboss","gloss_contour":{"preset":"cone_inverted"},
            "gloss_anti_aliased":true,"contour_enabled":true,
            "contour":{"preset":"half_round"},"contour_anti_aliased":true,"contour_range":0.3}
    ]});
    let style = LayerStyle::from_json(&json.to_string()).unwrap();
    let text = style.to_json();
    let again = LayerStyle::from_json(&text).unwrap();
    assert_eq!(again, style);
    assert_eq!(again.to_json(), text);
    let v: Value = serde_json::from_str(&text).unwrap();
    let ig = v["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "inner_glow")
        .unwrap();
    assert_eq!(
        ig["contour"]["points"][1],
        json!({"x":96.0,"y":230.0,"corner":true})
    );
    assert_eq!(ig["contour"]["points"][0], json!({"x":0.0,"y":40.0}));
    assert_eq!(ig["range"], json!(0.75));
    match style.effect(EffectKind::BevelEmboss) {
        Some(Effect::BevelEmboss(b)) => {
            assert_eq!(
                b.gloss_contour,
                Contour::Preset(ContourPreset::ConeInverted)
            );
            assert!(b.contour_enabled && b.contour_anti_aliased && b.gloss_anti_aliased);
            assert_eq!(b.contour_range, 0.3);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn points_clamp_quantize_sort_and_keep_the_first_of_a_duplicate() {
    let style = LayerStyle::from_json(
        r#"{"effects":[{"type":"drop_shadow","contour":{"points":[
            {"x":300,"y":-5},{"x":10.123456,"y":20},{"x":10.123456,"y":99,"corner":true}]}}]}"#,
    )
    .unwrap();
    let Some(Effect::DropShadow(d)) = style.effect(EffectKind::DropShadow) else {
        panic!("no drop shadow");
    };
    let Contour::Custom(curve) = d.contour else {
        panic!("not custom: {:?}", d.contour);
    };
    let points: Vec<(f32, f32, bool)> = curve
        .points()
        .iter()
        .map(|p| (p.x, p.y, p.corner))
        .collect();
    assert_eq!(points, vec![(10.1235, 20.0, false), (255.0, 0.0, false)]);
}

#[test]
fn malformed_contours_are_refused_by_name() {
    let refused = |effect: &str| {
        LayerStyle::from_json(&format!("{{\"effects\":[{effect}]}}"))
            .err()
            .unwrap_or_else(|| panic!("{effect} parsed"))
    };
    let points = "effects[0].contour.points must hold 2 to 16 points";
    assert_eq!(
        refused(r#"{"type":"drop_shadow","contour":{"points":[{"x":0,"y":0}]}}"#),
        points
    );
    assert_eq!(
        refused(r#"{"type":"drop_shadow","contour":{"points":[{"x":5,"y":0},{"x":5,"y":9}]}}"#),
        points
    );
    let seventeen: Vec<Value> = (0..17).map(|i| json!({"x": i * 10, "y": 0})).collect();
    assert_eq!(
        refused(&json!({"type":"outer_glow","contour":{"points":seventeen}}).to_string()),
        points
    );
    assert_eq!(
        refused(r#"{"type":"satin","contour":5}"#),
        "effects[0].contour must be an object"
    );
    assert_eq!(
        refused(r#"{"type":"bevel_emboss","gloss_contour":[]}"#),
        "effects[0].gloss_contour must be an object"
    );
    assert!(
        refused(r#"{"type":"inner_glow","contour":{"preset":"spiral"}}"#)
            .starts_with("effects[0].contour.preset must be one of linear, cone, cone_inverted")
    );
    let lenient = LayerStyle::from_json_lenient(
        r#"{"effects":[{"type":"inner_glow","contour":{"preset":"spiral"}}]}"#,
    )
    .unwrap();
    assert_eq!(
        lenient,
        LayerStyle::from_json(r#"{"effects":[{"type":"inner_glow"}]}"#).unwrap(),
        "an unknown preset reads as Linear when lenient"
    );
}

#[test]
fn ranges_clamp_to_one_to_a_hundred_percent() {
    let style = LayerStyle::from_json(
        r#"{"effects":[{"type":"outer_glow","range":0},{"type":"bevel_emboss","contour_range":7}]}"#,
    )
    .unwrap();
    let text = style.to_json();
    assert!(text.contains("\"range\":0.01"), "{text}");
    assert!(text.contains("\"contour_range\":1.0"), "{text}");
}

// ------------------------------------------------------- the renderers --

#[test]
fn a_custom_drop_shadow_is_its_spline_on_the_linear_twin() {
    let doc = scene();
    let twin = coverage(&doc, drop_shadow(24.0));
    for (points, aa) in [(&CU1, false), (&CU2, false), (&CU2, true)] {
        let got = coverage(
            &doc,
            with(
                drop_shadow(24.0),
                json!({"contour":{"points":points_json(points)},"anti_aliased":aa}),
            ),
        );
        let table = lut(&custom_values(points), None, Some(CAP_DS));
        assert_mapped(&twin, &got, &table, |_| true, "custom drop shadow");
    }
}

#[test]
fn every_preset_draws_its_measured_table() {
    let doc = rect_layer_doc((400, 400), (100, 100, 200, 200), [0, 0, 0, 255]);
    let twin = coverage(&doc, drop_shadow(40.0));
    let mut codes: Vec<u8> = twin.clone();
    codes.sort_unstable();
    codes.dedup();
    assert!(codes.len() >= 250, "the twin reaches {} codes", codes.len());
    let measured = tables();
    for (name, values) in &measured {
        let got = coverage(
            &doc,
            with(drop_shadow(40.0), json!({"contour":{"preset":name}})),
        );
        let table = lut(values, None, Some(CAP_DS));
        assert_mapped(&twin, &got, &table, |_| true, name);
    }
    assert_eq!(measured.len(), 10, "ten presets");
}

#[test]
fn a_below_contour_stops_at_its_extent_whatever_the_pad() {
    let doc = scene();
    let measured = tables();
    let table = lut(&measured["cone_inverted"], None, Some(CAP_DS));
    let ds = drop_shadow(16.0);
    let contoured = with(ds.clone(), json!({"contour":{"preset":"cone_inverted"}}));
    // A satin at opacity 0 that only widens the style's pad.
    let wide = json!({"type":"satin","opacity":0,"size":200,"distance":400});
    let render = |effects: Value| -> Vec<u8> {
        let style = json!({"fill_opacity":0,"effects":effects});
        styled(&doc, 1, &style.to_string())
            .flattened()
            .unwrap()
            .pixels()
            .map(|p| 255 - p.0[0])
            .collect()
    };
    for (what, twin, got) in [
        ("alone", render(json!([ds])), render(json!([contoured]))),
        (
            "padded",
            render(json!([ds, wide])),
            render(json!([contoured, wide])),
        ),
    ] {
        // The contour reads its own plane only: L of the twin at either pad.
        assert_mapped(&twin, &got, &table, |_| true, what);
        assert_eq!(
            got[0], 0,
            "{what}: a corner far beyond the reach stays clear"
        );
        assert!(
            got.iter().zip(&twin).all(|(&g, &t)| t > 0 || g == 0),
            "{what}: nothing past the plane's own extent"
        );
    }
}

#[test]
fn the_outer_glow_range_maps_the_glow_before_its_knock_out() {
    // Below Spread 100 % the outer glow runs the box-pair law (#2,
    // `style_fx_outer_glow`), which applies its Range in the plane before
    // the contour, as the inner glow's law does: the twin is the Linear glow
    // at the same Range and the capped table has none.
    let doc = scene();
    let measured = tables();
    for (contour, range) in [
        ("linear", 0.25),
        ("cone", 0.25),
        ("linear", 1.0),
        ("cone", 0.5),
    ] {
        let twin = coverage(&doc, with(outer_glow(20.0), json!({"range":range})));
        let got = coverage(
            &doc,
            with(
                outer_glow(20.0),
                json!({"contour":{"preset":contour},"range":range}),
            ),
        );
        let table = lut(&measured[contour], None, Some(CAP_OG));
        assert_mapped(&twin, &got, &table, |i| !inside(i), contour);
        assert!(
            got.iter().enumerate().all(|(i, &g)| !inside(i) || g == 0),
            "{contour}: nothing under the shape"
        );
    }
}

#[test]
fn interior_contours_map_every_pixel_before_the_coverage_multiply() {
    let doc = scene();
    let measured = tables();
    // `law`: the inner glow's measured law (#2, Size·(1 − Choke) > 0)
    // applies its Range before the contour, so the twin is the Linear glow at
    // the same Range and the table has none.
    for (base, contour, range, law) in [
        (inner_glow(16.0), "cone", Some(0.25), true),
        (inner_glow(16.0), "cone_inverted", Some(0.5), false),
    ] {
        let twin = match (law, range) {
            (true, Some(r)) => coverage(&doc, with(base.clone(), json!({"range":r}))),
            _ => coverage(&doc, base.clone()),
        };
        let mut extra = json!({"contour":{"preset":contour}});
        if let Some(r) = range {
            extra["range"] = json!(r);
        }
        let got = coverage(&doc, with(base, extra));
        let table = lut(&measured[contour], if law { None } else { range }, None);
        assert_mapped(&twin, &got, &table, inside, contour);
        assert!(got.iter().enumerate().all(|(i, &g)| inside(i) || g == 0));
    }
    // The inner shadow under its law (#2 part 4, IC1,
    // `style_fx_inner_shadow::CONTOUR_UNROUNDED`): the contour reads the
    // UNROUNDED matte and rounds once, so each pixel is the measured table
    // read piecewise linearly somewhere in the half code its Linear twin's
    // byte rounds from, no longer the table of that byte (the positive pin
    // against an oracle is `style_effect_kernels_578_p4_law.rs`).
    for contour in ["cone_inverted", "ring"] {
        let twin = coverage(&doc, inner_shadow(16.0));
        let got = coverage(
            &doc,
            with(inner_shadow(16.0), json!({"contour":{"preset":contour}})),
        );
        let values = &measured[contour];
        let table = lut(values, None, None);
        let (mut n, mut off_table) = (0, 0);
        for (i, (&a, &g)) in twin.iter().zip(&got).enumerate() {
            if !inside(i) {
                continue;
            }
            let c = f64::from(a);
            let reads = [c - 0.5, c, c + 0.5].map(|at| read(values, at.clamp(0.0, 255.0)));
            let lo = reads.iter().copied().fold(f64::INFINITY, f64::min).round();
            let hi = reads
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
                .round();
            assert!(
                (lo..=hi).contains(&f64::from(g)),
                "{contour}: pixel {i} twin {a} reads {g}, not in {lo} ..= {hi}"
            );
            off_table += usize::from(g != table[usize::from(a)]);
            n += 1;
        }
        assert!(n > 100, "{contour}: only {n} pixels judged");
        assert!(
            off_table > 0,
            "{contour}: every pixel is still the table of its twin's byte"
        );
        assert!(got.iter().enumerate().all(|(i, &g)| inside(i) || g == 0));
    }
}

#[test]
fn the_satin_contour_maps_each_shifted_copy_not_the_combined_plane() {
    // Since #2 part 3 the contour maps each shifted copy before the
    // combine (`style_fx_satin`), so the contoured satin is no longer a
    // table of its Linear twin: the contour-557 site this test pinned
    // (BEFORE-INVERT, the inverted twin q reading 255 − L[255 − q]) now
    // misses the render inside the shape. This test only asserts that miss;
    // the POSITIVE pin of the per-copy mapping, against these measured
    // tables at 8 and 16 bits, is `style_satin_law.rs`'s
    // `a_contoured_satin_maps_each_shifted_copy_before_the_combine`.
    let doc = scene();
    let measured = tables();
    let twin = coverage(&doc, satin());
    for contour in ["cone", "rolling_slope_descending"] {
        let got = coverage(&doc, with(satin(), json!({"contour":{"preset":contour}})));
        let table = lut(&measured[contour], None, None);
        let flipped: Vec<u8> = (0..256).map(|q| 255 - table[255 - q]).collect();
        let off = twin
            .iter()
            .zip(&got)
            .enumerate()
            .filter(|&(i, (&a, &g))| inside(i) && flipped[usize::from(a)].abs_diff(g) > 2)
            .count();
        assert!(
            off > 100,
            "{contour}: the combined-plane site still reads {off} pixels off"
        );
        assert!(got.iter().enumerate().all(|(i, &g)| inside(i) || g == 0));
    }
}

/// The highlight and the shadow of a bevel of `style` (Smooth unless `extra`
/// names a technique: a Smooth `emboss` or `inner_bevel` runs the measured
/// law of #2, an `emboss` with `"technique":"chisel_hard"` today's lateral
/// kernel), each read alone (the other side at opacity 0).
fn bevel_sides(doc: &DrDocument, style: &str, extra: Value) -> (Vec<u8>, Vec<u8>) {
    let base = json!({"type":"bevel_emboss","style":style,"size":12,"depth":1,
        "angle":0,"altitude":30,"use_global_light":false,
        "highlight_blend":"normal","highlight_color":"#000000",
        "shadow_blend":"normal","shadow_color":"#000000"});
    let side = |h: f64, s: f64| {
        coverage(
            doc,
            with(
                with(base.clone(), extra.clone()),
                json!({"highlight_opacity":h,"shadow_opacity":s}),
            ),
        )
    };
    (side(1.0, 0.0), side(0.0, 1.0))
}

/// The signed shading `d` of `bevel_sides`'s Smooth `style` under the
/// measured law of #2, written here from the law's definition (never the
/// kernel): the rect's coverage through the separable triangle
/// `(W − |j|) / W²` (W = 12 for an Inner Bevel at Size 12, 6 for an Emboss,
/// whose height is the law's at half the Size), reads clamped; the unit
/// height `h` through the byte contour `table` when there is one, read
/// piecewise linearly at `255 h` as the kernel reads its u8 table; the slope
/// `depth · W · (h[x+1] − h[x−1]) / 2`; and the NORMALIZED shading under the
/// light at angle 0, altitude 30, whose shadow side reaches `d` = −1. An
/// Inner Bevel's `d` is 0 outside the rect (its inner band); an Emboss draws
/// both bands, so its `d` is the law's everywhere.
fn bevel_law(style: &str, depth: f64, table: Option<&[u8]>) -> Vec<f64> {
    let big_w: i64 = match style {
        "inner_bevel" => 12,
        "emboss" => 6,
        other => panic!("no law oracle for {other}"),
    };
    let (w, h) = (CANVAS.0 as i64, CANVAS.1 as i64);
    let n = (w * h) as usize;
    let tri = |j: i64| (big_w - j.abs()) as f64 / (big_w * big_w) as f64;
    let pass = |src: &[f64], dx: i64, dy: i64| -> Vec<f64> {
        (0..n)
            .map(|i| {
                let (x, y) = (i as i64 % w, i as i64 / w);
                (1 - big_w..big_w)
                    .map(|j| {
                        let xx = (x + j * dx).clamp(0, w - 1);
                        let yy = (y + j * dy).clamp(0, h - 1);
                        tri(j) * src[(yy * w + xx) as usize]
                    })
                    .sum()
            })
            .collect()
    };
    let a: Vec<f64> = (0..n).map(|i| if inside(i) { 1.0 } else { 0.0 }).collect();
    let mut height = pass(&pass(&a, 1, 0), 0, 1);
    if let Some(t) = table {
        let t: Vec<f64> = t.iter().map(|&c| f64::from(c)).collect();
        for v in &mut height {
            *v = read(&t, 255.0 * v.clamp(0.0, 1.0)) / 255.0;
        }
    }
    let at = |x: i64, y: i64| height[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
    let half = depth * big_w as f64 / 2.0;
    let (lx, lz) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    (0..n)
        .map(|i| {
            if style == "inner_bevel" && !inside(i) {
                return 0.0;
            }
            let (x, y) = (i as i64 % w, i as i64 / w);
            let hx = (at(x + 1, y) - at(x - 1, y)) * half;
            let hy = (at(x, y + 1) - at(x, y - 1)) * half;
            let nl = (-hx * lx + lz) / (1.0 + hx * hx + hy * hy).sqrt();
            if nl >= lz {
                (nl - lz) / (1.0 - lz)
            } else {
                (nl - lz) / lz
            }
        })
        .collect()
}

/// `d`'s highlight and shadow bytes, `round(255 · clamp(±d))`.
fn law_sides(d: f64) -> (f64, f64) {
    (
        (255.0 * d.clamp(0.0, 1.0)).round(),
        (255.0 * (-d).clamp(0.0, 1.0)).round(),
    )
}

#[test]
fn the_gloss_contour_maps_the_signed_shading_before_it_splits() {
    let doc = scene();
    let measured = tables();
    let cone: Vec<f64> = measured["cone"].clone();
    // The Cone's byte table, the one the kernel reads, read piecewise
    // linearly (the law's oracle).
    let cone_bytes: Vec<f64> = lut(&cone, None, None)
        .iter()
        .map(|&c| f64::from(c))
        .collect();
    for (what, style, technique) in [
        ("emboss", "emboss", json!({})),
        ("inner_bevel", "inner_bevel", json!({})),
        (
            "chisel_hard emboss",
            "emboss",
            json!({"technique":"chisel_hard"}),
        ),
    ] {
        let lateral = what == "chisel_hard emboss";
        let (gh, gs) = bevel_sides(
            &doc,
            style,
            with(
                technique.clone(),
                json!({"gloss_contour":{"preset":"cone"}}),
            ),
        );
        // Chisel Hard Emboss (today's lateral kernel): `d` rebuilt from the
        // Linear twin's bytes, within ±0.5 / 255, which the cone's slope 2 on
        // the lateral shading doubles. The law (a Smooth `emboss` and
        // `inner_bevel`): `d` from `bevel_law`, whose NORMALIZED shadow side
        // reaches −1, where the cone's slope is 3, so the oracle maps `d`
        // itself, through the byte table, and leaves only the output
        // rounding. Within 1.5 codes.
        let want: Vec<(f64, f64)> = if lateral {
            let (h, s) = bevel_sides(&doc, style, technique.clone());
            (0..h.len())
                .map(|i| {
                    let d = (f64::from(h[i]) - f64::from(s[i])) / 255.0;
                    let d2 = if h[i].max(s[i]) < GLOSS_SUPPORT {
                        0.0
                    } else {
                        2.0 * read(&cone, 255.0 * (d + 1.0) / 2.0) / 255.0 - 1.0
                    };
                    (255.0 * d2.clamp(0.0, 1.0), 255.0 * (-d2).clamp(0.0, 1.0))
                })
                .collect()
        } else {
            bevel_law(style, 1.0, None)
                .into_iter()
                .map(|d| {
                    let (bh, bs) = law_sides(d);
                    let d2 = if bh.max(bs) < f64::from(GLOSS_SUPPORT) {
                        0.0
                    } else {
                        let at = 255.0 * (d.clamp(-1.0, 1.0) + 1.0) / 2.0;
                        2.0 * read(&cone_bytes, at) / 255.0 - 1.0
                    };
                    law_sides(d2)
                })
                .collect()
        };
        let mut worst = 0.0f64;
        let mut total = 0.0;
        let mut n = 0;
        for (i, &(wh, ws)) in want.iter().enumerate() {
            let e = (f64::from(gh[i]) - wh)
                .abs()
                .max((f64::from(gs[i]) - ws).abs());
            worst = worst.max(e);
            total += e;
            n += 1;
        }
        assert!(worst <= 1.5, "{what}: worst {worst}");
        assert!(
            total / f64::from(n) <= 0.5,
            "{what}: mean {}",
            total / f64::from(n)
        );
        assert_eq!(
            gh[0], 0,
            "{what}: flat ground outside the support stays clear"
        );
    }
}

/// Under today's kernel (a Chisel Hard `emboss`) the halving contour halves
/// the u8 height, so it shades as its own Depth 0.5 twin does. Under the
/// measured law (a Smooth `emboss` and `inner_bevel`) it halves the unit
/// height `h` before the shading (`map_unit_plane_with_job`) through the u8
/// table `round(c / 2)`, whose rounding the law's slope gain `depth · W`
/// amplifies; the twin is then `bevel_law` at Depth 1 through that same byte
/// table, so the rounding is the same on both sides.
#[test]
fn a_halving_bevel_contour_shades_like_half_the_depth() {
    let doc = scene();
    let half = json!({"contour_enabled":true,
        "contour":{"points":[{"x":0,"y":0},{"x":255,"y":127.5}]}});
    // The bevel Contour's byte table `L[c] = round(C(255 R(c / 255)))` at its
    // default Range 50 %, under the bevel's Range rule (the take's BEVEL best
    // fit) `R(x) = max(0, 1 − (1 − x) / 2r)`: the identity at 0.5 but for its
    // f64 rounding, which decides which half codes `c / 2` round up.
    let halving_curve = custom_values(&[(0.0, 0.0, false), (255.0, 127.5, false)]);
    let halving: Vec<u8> = (0..256)
        .map(|c| {
            let x = c as f64 / 255.0;
            let x = (1.0 - (1.0 - x) / (2.0 * 0.5)).max(0.0);
            read(&halving_curve, 255.0 * x).round().clamp(0.0, 255.0) as u8
        })
        .collect();
    for (what, style, technique) in [
        ("emboss", "emboss", json!({})),
        ("inner_bevel", "inner_bevel", json!({})),
        (
            "chisel_hard emboss",
            "emboss",
            json!({"technique":"chisel_hard"}),
        ),
    ] {
        let lateral = what == "chisel_hard emboss";
        let (h, s) = bevel_sides(&doc, style, with(technique.clone(), half.clone()));
        let (dh, ds) = if lateral {
            bevel_sides(&doc, style, with(technique.clone(), json!({"depth":0.5})))
        } else {
            bevel_law(style, 1.0, Some(&halving))
                .into_iter()
                .map(|d| {
                    let (bh, bs) = law_sides(d);
                    (bh as u8, bs as u8)
                })
                .unzip()
        };
        let errors: Vec<f64> = h
            .iter()
            .zip(&dh)
            .chain(s.iter().zip(&ds))
            .map(|(&a, &b)| (f64::from(a) - f64::from(b)).abs())
            .collect();
        let worst = errors.iter().cloned().fold(0.0, f64::max);
        let mean = errors.iter().sum::<f64>() / errors.len() as f64;
        assert!(
            worst <= BOUND_MAX && mean <= BOUND_MEAN,
            "{what}: worst {worst}, mean {mean}"
        );
        let (lh, _) = bevel_sides(
            &doc,
            style,
            with(technique.clone(), json!({"contour_enabled":true})),
        );
        let (bh, _) = bevel_sides(&doc, style, technique.clone());
        assert_eq!(lh, bh, "{what}: Linear at Range 50 % is the identity");
        let (ch, _) = bevel_sides(
            &doc,
            style,
            with(
                technique.clone(),
                json!({"contour_enabled":true,"contour":{"preset":"cone"}}),
            ),
        );
        assert_ne!(ch, bh, "{what}: a Cone reshapes the bevel");
        let (off, _) = bevel_sides(
            &doc,
            style,
            with(technique.clone(), json!({"contour":{"preset":"cone"}})),
        );
        assert_eq!(
            off, bh,
            "{what}: a contour with the sub-panel off draws nothing new"
        );
    }
}

/// A contour with `C(0) > 0` lifts the zero ground to a flat plateau that
/// reaches the plane border. A Smooth Outer Bevel runs the measured law
/// (#2, `style_bevel_smooth`): the contour maps the unit height `h` before
/// the shading, and Soften is a box pair on the shading, not a blur of the
/// height, so the plateau stays flat, shades to nothing, and the bevel draws
/// no line along the plane's edge. The rect is 121 x 81 px, so the plane's
/// width and height are odd.
#[test]
fn a_lifting_bevel_contour_draws_nothing_at_the_plane_border() {
    let (x0, y0, w, h) = (140, 110, 121u32, 81u32);
    let canvas = (400u32, 300u32);
    let doc = rect_layer_doc(canvas, (x0, y0, w, h), [0, 0, 0, 255]);
    let bevel = json!({"type":"bevel_emboss","style":"outer_bevel","size":12,"depth":1,
        "soften":8,"angle":45,"altitude":30,"use_global_light":false,
        "highlight_blend":"normal","highlight_color":"#000000",
        "shadow_blend":"normal","shadow_color":"#000000",
        "contour_enabled":true,"contour":{"preset":"cone_inverted"}});
    let got = coverage(&doc, bevel);
    // Nothing past the band may draw. The law's own reach is
    // `smooth_reach(12, 8)` = 12 + 1 + 7 = 20; the bound stays today's 34
    // (the size's blur and the soften's, 3 sigma each, plus the stencil).
    let band = 18 + 12 + 2 + 2;
    let far = |i: usize| {
        let (x, y) = ((i as u32 % canvas.0) as i32, (i as u32 / canvas.0) as i32);
        let dx = (x0 - x).max(x - (x0 + w as i32 - 1)).max(0);
        let dy = (y0 - y).max(y - (y0 + h as i32 - 1)).max(0);
        dx.max(dy) > band
    };
    let stray: Vec<usize> = (0..got.len()).filter(|&i| far(i) && got[i] != 0).collect();
    assert!(
        stray.is_empty(),
        "{} stray pixels, first at {:?}",
        stray.len(),
        stray
            .first()
            .map(|&i| (i as u32 % canvas.0, i as u32 / canvas.0))
    );
    assert!(got.iter().any(|&a| a != 0), "the bevel draws");
}

// -------------------------------------------------------- Photoshop's rows --

/// Doctored's own table for `effect` (the contoured effect against its
/// Linear twin, `base`), at every twin code the scene reaches.
/// Its shape: the rect (100, 100, 200, 200) of a 400 x 400 canvas.
fn observed(doc: &DrDocument, base: Value, extra: Value, below: bool) -> BTreeMap<u8, u8> {
    let twin = coverage(doc, base.clone());
    let got = coverage(doc, with(base, extra));
    let within = |i: usize| {
        let (x, y) = (i % 400, i / 400);
        (100..300).contains(&x) && (100..300).contains(&y)
    };
    let mut out = BTreeMap::new();
    for (i, (&a, &g)) in twin.iter().zip(&got).enumerate() {
        if below == within(i) || a == 0 {
            continue;
        }
        out.insert(a, g);
    }
    out
}

#[test]
fn photoshops_readings_hold_to_the_takes_bound() {
    let text = std::fs::read_to_string(fixture("rows.json")).expect("rows.json");
    let rows: BTreeMap<String, BTreeMap<String, f64>> = serde_json::from_str(&text).unwrap();
    let doc = rect_layer_doc((400, 400), (100, 100, 200, 200), [0, 0, 0, 255]);
    let rows_of = |name: &str| rows.get(name).unwrap_or_else(|| panic!("no row {name}"));
    let mut checked = 0;
    for (row, base, extra, below) in [
        (
            "ds_cu1",
            drop_shadow(40.0),
            json!({"contour":{"points":points_json(&CU1)}}),
            true,
        ),
        (
            "ds_cu2",
            drop_shadow(40.0),
            json!({"contour":{"points":points_json(&CU2)}}),
            true,
        ),
        (
            "og_cone",
            outer_glow(40.0),
            json!({"contour":{"preset":"cone"}}),
            true,
        ),
        (
            "ds_cone_inverted",
            drop_shadow(40.0),
            json!({"contour":{"preset":"cone_inverted"}}),
            true,
        ),
        (
            "og_cone_inverted",
            outer_glow(40.0),
            json!({"contour":{"preset":"cone_inverted"}}),
            true,
        ),
        (
            "og_cone_r25",
            outer_glow(40.0),
            json!({"contour":{"preset":"cone"},"range":0.25}),
            true,
        ),
        (
            "is_cone",
            inner_shadow(40.0),
            json!({"contour":{"preset":"cone"}}),
            false,
        ),
        (
            "ig_cone",
            inner_glow(40.0),
            json!({"contour":{"preset":"cone"}}),
            false,
        ),
    ] {
        let ours = observed(&doc, base, extra, below);
        let theirs = rows_of(row);
        let mut worst = 0.0f64;
        let mut total = 0.0;
        let mut n = 0;
        for (code, reading) in theirs {
            let code: u8 = code.parse().unwrap();
            let Some(&g) = ours.get(&code) else {
                continue;
            };
            let e = (f64::from(g) - reading).abs();
            worst = worst.max(e);
            total += e;
            n += 1;
        }
        assert!(n >= 20, "{row}: only {n} shared codes");
        let mean = total / f64::from(n);
        assert!(
            worst <= BOUND_MAX && mean <= BOUND_MEAN,
            "{row}: worst {worst}, mean {mean} over {n} codes"
        );
        checked += 1;
    }
    assert_eq!(checked, 8);
}

// ------------------------------------------------------------------ .rz 96 --

#[test]
fn the_byte_word_96_comes_first_round_trips_and_refuses_both_ways() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = scene();
    let save = |name: &str, style: &Value| -> (std::path::PathBuf, Vec<u8>, DrDocument) {
        let styled_doc = styled(&doc, 1, &style.to_string());
        let path = dir.path().join(name);
        styled_doc.save_native(path.to_str().unwrap()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        (path, bytes, styled_doc)
    };
    let (_, plain, _) = save("plain.rz", &json!({"effects":[{"type":"inner_glow"}]}));
    assert_ne!(
        &plain[8..12],
        &96u32.to_le_bytes(),
        "no 96 without a contour"
    );
    for (name, style) in [
        (
            "contour",
            json!({"effects":[{"type":"inner_glow","contour":{"preset":"ring"}}]}),
        ),
        (
            "range",
            json!({"effects":[{"type":"outer_glow","range":0.01}]}),
        ),
        (
            "panel",
            json!({"effects":[{"type":"bevel_emboss","contour_enabled":true}]}),
        ),
    ] {
        let (path, bytes, styled_doc) = save(&format!("{name}.rz"), &style);
        assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
        assert_eq!(
            &bytes[8..12],
            &96u32.to_le_bytes(),
            "{name}: FIRST inside 45"
        );
        assert_eq!(&bytes[12..16], &plain[8..12], "{name}: then the plain word");
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
        let cut = dir.path().join(format!("{name}-cut.rz"));
        std::fs::write(&cut, &stripped).unwrap();
        let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
            panic!("{name}: a contour without the word must refuse");
        };
        assert!(
            message.ends_with("native layer style contours require DRDC 96"),
            "{message}"
        );
    }
    // With a pattern stroke too, 96 comes before 95.
    let (_, both, _) = save(
        "both.rz",
        &json!({"effects":[{"type":"satin","contour":{"preset":"cone"}},
            {"type":"stroke","pattern_fill":{"scale":2}}]}),
    );
    assert_eq!(&both[8..12], &96u32.to_le_bytes());
    assert_eq!(&both[12..16], &95u32.to_le_bytes(), "96 ahead of 95");
    let mut forged = plain.clone();
    forged.splice(8..8, 96u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = DrDocument::open(path.to_str().unwrap()) else {
        panic!("the word without a contour must refuse");
    };
    assert!(
        message.ends_with("invalid native contour word"),
        "{message}"
    );
}
