//! The `curves` adjustment's MAP form (`rgb_map`, `r_map`,
//! `g_map`, `b_map`; `core/src/adjust_curves.rs`), black-box through
//! `dr_image_adjust_op`, the compositor and the typed `PrecisionAdjustment`.
//!
//! The oracles are written out here from the schema: at 8 bits
//! `out = rgb_map[channel_map[v]]` exactly; at 16 bits Photoshop's
//! all-integer 15-bit stage tables, the channel's map first and the
//! composite's second, from the rule text alone.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::DrDocument;
use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::icc::IccProfile;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::*;

/// `dr_image_adjust_op("curves", params)`, or the core's refusal.
fn curves(img: *const DrImage, params: &Value) -> Result<*mut DrImage, String> {
    let op = CString::new("curves").unwrap();
    let params = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(img, op.as_ptr(), params.as_ptr(), &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        assert!(err.is_null(), "success must not set err_out");
        Ok(out)
    }
}

fn typed(params: Value) -> Result<PrecisionAdjustment, String> {
    let text = json!({"type": "adjust", "evaluation": "precision_v1", "op": "curves",
                      "params": params})
    .to_string();
    PrecisionAdjustment::parse(&text, &IccProfile::srgb())
}

/// A non-monotone composite (a square root with a notch, as a pencil draws)
/// and two channel maps.
fn composite() -> Vec<u8> {
    (0..256)
        .map(|i| {
            if (96..112).contains(&i) {
                32
            } else {
                (255.0 * (f64::from(i) / 255.0).sqrt()).round() as u8
            }
        })
        .collect()
}
fn red() -> Vec<u8> {
    (0..256).map(|i| 255 - i as u8).collect()
}
fn green() -> Vec<u8> {
    (0..256)
        .map(|i| {
            if (100..=115).contains(&i) {
                200
            } else {
                i as u8
            }
        })
        .collect()
}

// ---- the 16-bit oracle, written from the rule text alone --------
//
// R(n, d) = floor((2n + d) / (2d)); q_i = R(32768 m_i, 255);
// S[j] = q_i + R((q_{i+1} - q_i)(255 j - 4096 i), 4096), i = min(floor(255 j / 4096), 254);
// read(A, x) = A[i] + R((A[i+1] - A[i])(x - 8 i), 8), i = min(floor(x / 8), 4095);
// T_c[j] = read(S_master, S_channel[j]); a stored code u reads T_c at
// k = R(32768 u, 65535) and writes R(65535 read, 32768).

fn half_up(n: i64, d: i64) -> i64 {
    (2 * n + d).div_euclid(2 * d)
}

fn stage(map: &[u8]) -> Vec<i64> {
    let q: Vec<i64> = map
        .iter()
        .map(|&m| half_up(32768 * i64::from(m), 255))
        .collect();
    (0..=4096i64)
        .map(|j| {
            let i = (255 * j / 4096).min(254);
            let iu = i as usize;
            q[iu] + half_up((q[iu + 1] - q[iu]) * (255 * j - 4096 * i), 4096)
        })
        .collect()
}

fn read(table: &[i64], x: i64) -> i64 {
    let x = x.clamp(0, 32768);
    let i = (x / 8).min(4095);
    let iu = i as usize;
    table[iu] + half_up((table[iu + 1] - table[iu]) * (x - 8 * i), 8)
}

/// One channel's composed table `T_c` under `master`.
fn composed(master: &[u8], channel: &[u8]) -> Vec<i64> {
    let s_master = stage(master);
    stage(channel).iter().map(|&x| read(&s_master, x)).collect()
}

/// The 16-bit code for stored code `u` through a composed 15-bit stage table.
fn staged16(table: &[i64], u: u16) -> u16 {
    let k = half_up(32768 * i64::from(u), 65535);
    half_up(65535 * read(table, k), 32768) as u16
}

/// The map params `rgb_map`/`r_map`/`g_map`/`b_map` with the identity for an
/// absent channel, in r, g, b, master order.
fn maps_of(params: &Value) -> [Vec<u8>; 4] {
    let get = |key: &str| -> Vec<u8> {
        match params.get(key) {
            Some(list) => list
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect(),
            None => (0..=255).collect(),
        }
    };
    [get("r_map"), get("g_map"), get("b_map"), get("rgb_map")]
}

/// The take sets' maps (`scripts/photoshop-probe/float_adjust_take4.py`,
/// `float_adjust_take5.py`): M2 i ± 24 alternating, M4 0/255 alternating, M5's
/// red i + 40((i mod 3) − 1), M7's green 255 − i, all clamped to 0…255.
fn m2() -> Vec<u8> {
    (0..256i32)
        .map(|i| (i + 24 * (2 * (i % 2) - 1)).clamp(0, 255) as u8)
        .collect()
}
fn m4() -> Vec<u8> {
    (0..256).map(|i| if i % 2 == 1 { 255 } else { 0 }).collect()
}
fn m5_red() -> Vec<u8> {
    (0..256i32)
        .map(|i| (i + 40 * ((i % 3) - 1)).clamp(0, 255) as u8)
        .collect()
}

/// The committed Photoshop take's three maps (`curves_map/description.json`).
fn fixture_params() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/curves_map/description.json");
    let description: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let params = &description["layers"][0]["params"];
    let mut out = serde_json::Map::new();
    out.insert("rgb_map".into(), params["mapping"].clone());
    for entry in params["channels"].as_array().unwrap() {
        let key = match entry["channel"].as_str().unwrap() {
            "red" => "r_map",
            "green" => "g_map",
            _ => "b_map",
        };
        out.insert(key.into(), entry["mapping"].clone());
    }
    Value::Object(out)
}

/// `apply_at` on a stored 16-bit triple, as the 16-bit codes it writes.
fn codes(adjustment: &PrecisionAdjustment, stored: [u16; 3]) -> [u16; 3] {
    let got = adjustment.apply_at(stored.map(|u| f32::from(u) / 65535.0), 0.0, (0, 0));
    got.map(|v| {
        let code = (f64::from(v) * 65535.0).round();
        assert_eq!(v, code as f32 / 65535.0, "{v} is not a 16-bit code");
        code as u16
    })
}

#[test]
fn the_8_bit_map_is_the_exact_table_with_the_channel_first() {
    let dir = tempfile::tempdir().unwrap();
    let src = test_pattern(64, 48);
    let img = open_pattern(&dir, "pattern.png", &src);
    let (rgb, r, g) = (composite(), red(), green());
    let out = curves(img, &json!({"rgb_map": rgb, "r_map": r, "g_map": g})).expect("curves");
    let got = pixels(out);
    let identity: Vec<u8> = (0..=255).collect();
    let mut reversed_differs = false;
    for (i, px) in src.pixels().enumerate() {
        for (c, table) in [&r, &g, &identity].into_iter().enumerate() {
            let v = usize::from(px[c]);
            let want = rgb[usize::from(table[v])];
            assert_eq!(got[i * 4 + c], want, "pixel {i} channel {c}");
            reversed_differs |= table[usize::from(rgb[v])] != want;
        }
        assert_eq!(got[i * 4 + 3], px[3], "alpha changed at {i}");
    }
    assert!(reversed_differs, "the composite-first order must differ");
    free(out);
    free(img);
}

/// At 16 bits the maps are read through 15-bit stage tables
/// (measured against Photoshop: receipt `float-adjust-473/take5`, CM16X
/// `int15`, exact on every sample), checked against the oracle above at every 16-bit code of every
/// channel of the committed fixture's maps, and against Photoshop's own codes.
#[test]
fn the_typed_map_reads_through_15_bit_stage_tables() {
    let params = fixture_params();
    let adjustment = typed(params.clone()).expect("parse");
    assert!(!adjustment.is_identity());
    let [r, g, b, master] = maps_of(&params);
    let tables = [&r, &g, &b].map(|channel| composed(&master, channel));
    for u in 0..=65535u16 {
        let got = codes(&adjustment, [u; 3]);
        for c in 0..3 {
            assert_eq!(got[c], staged16(&tables[c], u), "code {u} channel {c}");
        }
    }
    // Photoshop 26.10.0's own 16-bit codes: (set, stored, Photoshop's).
    // take4 (`float-adjust-473/take4`, map16): M5 at a stored red of 65535 on
    // srgb16 and on wide16, M4 and M6 on srgb16; take5's M7 on srgb16.
    let identity: Vec<u8> = (0..=255).collect();
    let rows: [(&str, [u16; 3], [u16; 3]); 9] = [
        ("M5", [65535, 65535, 65535], [61039, 65535, 65535]),
        ("M5", [65535, 65535, 65535], [61039, 65535, 65535]),
        ("M5", [32768, 13107, 6554], [37911, 19008, 6536]),
        ("M4", [32768, 13107, 6554], [32768, 64107, 32666]),
        ("M4", [48192, 31753, 22884], [31808, 29194, 62819]),
        ("M6", [32768, 13107, 6554], [46389, 29298, 20690]),
        ("M6", [47824, 31533, 23011], [34159, 45411, 38807]),
        ("M7", [32768, 13107, 6554], [32787, 1428, 32666]),
        ("M7", [48192, 31753, 22884], [6422, 36341, 62819]),
    ];
    for (set, stored, theirs) in rows {
        let set_params = match set {
            "M4" => json!({"rgb_map": m4()}),
            "M5" => json!({"rgb_map": m2(), "r_map": m5_red()}),
            "M6" => fixture_params(),
            _ => json!({"rgb_map": m4(), "r_map": m5_red(),
                        "g_map": identity.iter().rev().copied().collect::<Vec<u8>>()}),
        };
        let adjustment = typed(set_params).expect("parse");
        assert_eq!(codes(&adjustment, stored), theirs, "{set} at {stored:?}");
    }
}

#[test]
fn float_inputs_outside_the_unit_range_clamp_to_the_ends() {
    let params = json!({"rgb_map": composite(), "r_map": red()});
    let adjustment = typed(params.clone()).expect("parse");
    let low = adjustment.apply_at([-0.5, -3.0, -0.01], 0.0, (0, 0));
    let high = adjustment.apply_at([1.5, 4.0, 1.01], 0.0, (0, 0));
    let [r, g, b, master] = maps_of(&params);
    let tables = [&r, &g, &b].map(|channel| composed(&master, channel));
    let at = |c: usize, u: u16| f32::from(staged16(&tables[c], u)) / 65535.0;
    assert_eq!(low, [at(0, 0), at(1, 0), at(2, 0)]);
    assert_eq!(high, [at(0, 65535), at(1, 65535), at(2, 65535)]);
}

#[test]
fn identity_maps_are_identity_at_high_depth() {
    let identity: Vec<u8> = (0..=255).collect();
    assert!(typed(json!({"rgb_map": identity.clone()}))
        .expect("parse")
        .is_identity());
    assert!(
        typed(json!({"rgb_map": identity.clone(), "b_map": identity}))
            .expect("parse")
            .is_identity()
    );
    assert!(!typed(json!({"g_map": green()}))
        .expect("parse")
        .is_identity());
}

#[test]
fn malformed_maps_refuse_at_both_depths() {
    let dir = tempfile::tempdir().unwrap();
    let img = open_pattern(&dir, "p.png", &test_pattern(8, 8));
    let with = |at: usize, value: Value| {
        let mut list: Vec<Value> = (0..256).map(|i| json!(i)).collect();
        list[at] = value;
        Value::Array(list)
    };
    let short: Vec<u8> = (0..255).collect();
    let long: Vec<u16> = (0..257).collect();
    let refused = [
        json!({"rgb_map": short}),
        json!({"r_map": long}),
        json!({"g_map": with(7, json!(12.5))}),
        json!({"b_map": with(200, json!(256))}),
        json!({"rgb_map": with(0, json!(-1))}),
        json!({"rgb_map": with(3, json!("3"))}),
        json!({"rgb_map": "identity"}),
        json!({"rgb": [[0, 0], [255, 255]], "rgb_map": with(0, json!(0))}),
        json!({"r": [[0, 10], [255, 245]], "g_map": green()}),
    ];
    for params in refused {
        assert!(curves(img, &params).is_err(), "{params} must refuse");
        assert!(typed(params.clone()).is_err(), "typed {params} must refuse");
    }
    // A zero-fraction float is an integer.
    let floats = with(9, json!(9.0));
    let out = curves(img, &json!({"rgb_map": floats})).expect("9.0 is accepted");
    free(out);
    free(img);
}

/// The layer and the filter run one kernel: a map-form curves LAYER over an
/// image flattens to exactly the destructive op's bytes.
#[test]
fn a_map_curves_layer_matches_the_destructive_op() {
    let dir = tempfile::tempdir().unwrap();
    let src = opaque_pattern(24, 16);
    let img = open_pattern(&dir, "pattern.png", &src);
    let params = json!({"rgb_map": composite(), "b_map": red(), "g_map": green()});
    let out = curves(img, &params).expect("curves");
    let mut doc = DrDocument::from_pixels(src.clone());
    doc = doc
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(24, 16, Rgba([255, 0, 255, 255])),
            "Curves",
        )
        .expect("add layer");
    doc.layers[1].meta =
        Some(json!({"type": "adjust", "op": "curves", "params": params}).to_string());
    assert_eq!(doc.flattened().expect("flatten").as_raw(), &pixels(out));
    free(out);
    free(img);
}
