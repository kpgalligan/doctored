//! The style library EXPORT keeps Photoshop's record layout (`.asl`).
//!
//! Photoshop 26.10 refused Doctored's `open/open.asl` with "program error".
//! The oracle is `common/asl_layout.rs`, an independent walker holding the
//! layout rules Photoshop's own libraries keep (`styles_a.asl`,
//! `styles_b.asl`); every library `dr_asl_encode` writes must keep them all
//! and read back through `dr_asl_decode` as its canonical presets.
//!
//! `writes_the_hand_load_files_when_asked` writes the owner's hand-load
//! files (`ASL_HAND_DIR`, `ASL_HAND_TAG`, `ASL_HAND_SINGLES`,
//! `ASL_HAND_BLENDING`) and does nothing without them.

use std::collections::BTreeSet;
use std::ffi::{c_char, CString};
use std::path::PathBuf;
use std::ptr;

use doctored_core::ffi_style::{dr_asl_decode, dr_asl_encode, dr_style_preset_canonical};
use serde_json::{json, Value};

use crate::asl_layout::violations;
use crate::common::take_err_string;

// --------------------------------------------------------------- the files --

fn repo(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/styles_asl")
        .join(name)
}

/// `rows.json`'s presets R1..R8 as `dr_asl_encode` input (their `keys`
/// dropped), named `<prefix>R<n>` when a prefix is given.
pub(crate) fn take_presets(prefix: Option<&str>) -> Vec<Value> {
    let text = std::fs::read_to_string(repo("rows.json")).expect("rows.json");
    let rows: Value = serde_json::from_str(&text).unwrap();
    (1..=8)
        .map(|n| {
            let row = &rows["presets"][format!("R{n}")];
            let name = prefix.map_or_else(|| row["name"].clone(), |p| json!(format!("{p}R{n}")));
            json!({"name": name, "style": row["style"], "blending": row["blending"]})
        })
        .collect()
}

// ------------------------------------------------------------- C plumbing --

pub(crate) fn encode(presets: &Value) -> Vec<u8> {
    let c = CString::new(presets.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_encode(c.as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let out: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    assert_eq!(out["skipped"], json!([]), "{out}");
    base64(out["asl"].as_str().unwrap())
}

pub(crate) fn decode(bytes: &[u8]) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_decode(bytes.as_ptr(), bytes.len(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

pub(crate) fn canonical(style: &Value) -> Value {
    let c = CString::new(style.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_style_preset_canonical(c.as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

/// Standard base64 decoded, written here so the test does not lean on the
/// crate's codec.
fn base64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            let v = if c == b'=' {
                0
            } else {
                ALPHABET.iter().position(|&a| a == c).expect("base64") as u32
            };
            n |= v << (18 - 6 * i);
        }
        out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
    }
    out
}

fn rule_ids(bytes: &[u8]) -> BTreeSet<&'static str> {
    violations(bytes).into_iter().map(|(id, _)| id).collect()
}

// ---------------------------------------------------------------- presets --

/// A style with every effect kind, each kind Photoshop stacks held twice.
fn stacked_preset() -> Value {
    json!({"name": "Every kind stacked", "blending": null, "style": {"effects": [
        {"type":"drop_shadow","color":"#112233","opacity":0.5,"angle":45,
         "use_global_light":true,"distance":6,"size":9},
        {"type":"drop_shadow","color":"#445566","opacity":0.3,"angle":-60,
         "use_global_light":false,"distance":2,"size":3,"noise":0.2},
        {"type":"inner_shadow","color":"#223344","angle":45,"use_global_light":true,
         "distance":3,"choke":0.15,"size":5},
        {"type":"inner_shadow","color":"#663300","angle":90,"distance":1,"size":2},
        {"type":"stroke","position":"inside","size":2,"color":"#d5d9de"},
        {"type":"stroke","position":"outside","size":4,"fill_type":"gradient",
         "gradient":{"stops":[{"position":0,"color":"#ff0000","opacity":1},
                              {"position":1,"color":"#0000ff","opacity":0.25}],
                     "style":"reflected","angle":30,"scale":1.4}},
        {"type":"color_overlay","blend":"soft_light","color":"#00ff88","opacity":0.7},
        {"type":"color_overlay","color":"#8800ff","opacity":0.2},
        {"type":"gradient_overlay","opacity":0.5},
        {"type":"gradient_overlay","opacity":0.6,"angle":45},
        {"type":"pattern_overlay","pattern":{"preset":"dots"},"scale":1.5,"angle":20},
        {"type":"outer_glow","color":"#ffee00","opacity":0.4,"spread":0.1,"size":11,
         "contour":{"preset":"cone"},"range":0.4},
        {"type":"inner_glow","color":"#ccddee","choke":0.2,"size":7,"source":"center",
         "jitter":0.3},
        {"type":"bevel_emboss","style":"pillow_emboss","depth":2.5,"size":8,"soften":3},
        {"type":"satin","color":"#334455","opacity":0.45,"angle":25,"distance":12,
         "size":18,"invert":false}]}})
}

/// Blending, fill, knockout, Blend Interior and a multichannel Blend If.
fn blending_preset() -> Value {
    json!({"name": "Every blending option", "blending": {"mode":"multiply","opacity":0.7},
        "style": {"fill_opacity": 0.4, "knockout": "shallow", "blend_interior_as_group": true,
            "blend_if": {"channel":"gray","this_layer":[10,40,200,230],
                "underlying":[20,60,180,240],
                "extra":[{"channel":"red","this_layer":[5,30,220,250],
                          "underlying":[3,25,235,252]}]},
            "effects": [{"type":"drop_shadow"}]}})
}

/// No blending, but fill, knockout and Blend If. The earlier writer gave it
/// a `blendOptions` without `Opct`/`Md  ` (`hand/H11.asl`), which crashed
/// Photoshop 26.10; the writer now writes Normal 100 % for it.
pub(crate) fn no_blending_preset() -> Value {
    json!({"name": "Fill, knockout and Blend If", "blending": null,
        "style": {"fill_opacity": 0.5, "knockout": "deep",
            "blend_if": {"channel":"gray","this_layer":[0,0,200,240],
                "underlying":[30,60,255,255]},
            "effects": [{"type":"color_overlay","color":"#3366cc","opacity":0.8}]}})
}

fn one_effect_preset() -> Value {
    json!({"name": "One effect", "blending": null,
        "style": {"effects": [{"type":"outer_glow","size":6}]}})
}

/// Blend If alone: [`no_blending_preset`]'s Blend If and effects, without
/// its fill and knockout.
pub(crate) fn blend_if_only_preset() -> Value {
    let style = &no_blending_preset()["style"];
    json!({"name": "Blend If only", "blending": null,
        "style": {"blend_if": style["blend_if"], "effects": style["effects"]}})
}

/// Blending alone: no fill, knockout, Blend Interior or Blend If, so its
/// `Blnd` is the empty list.
pub(crate) fn blending_only_preset() -> Value {
    json!({"name": "Blending only", "blending": {"mode":"multiply","opacity":0.7},
        "style": {"effects": [{"type":"drop_shadow"}]}})
}

fn hand_fixture(name: &str) -> PathBuf {
    repo("hand").join(name)
}

/// The `blending` a written preset reads back with: its own, else Normal
/// 100 % when its style carries a blending option (the writer's full
/// `blendOptions` has no "style options without layer blending"), else
/// null.
pub(crate) fn written_blending(preset: &Value) -> Value {
    if !preset["blending"].is_null() {
        return preset["blending"].clone();
    }
    let style = canonical(&preset["style"]);
    let fill = style
        .get("fill_opacity")
        .and_then(Value::as_f64)
        .is_some_and(|v| v != 1.0);
    let blend_if = style.get("blend_if").is_some_and(|v| !v.is_null());
    let knockout = style.get("knockout").is_some();
    let interior = style.get("blend_interior_as_group").is_some();
    if fill || blend_if || knockout || interior {
        json!({"mode": "normal", "opacity": 1.0})
    } else {
        Value::Null
    }
}

// ------------------------------------------------------------------ tests --

#[test]
fn photoshops_libraries_keep_every_rule() {
    for file in ["styles_a.asl", "styles_b.asl"] {
        let bytes = std::fs::read(repo(file)).unwrap();
        assert_eq!(violations(&bytes), vec![], "{file}");
    }
}

#[test]
fn the_take_library_breaks_exactly_the_fixed_rules() {
    let bytes = std::fs::read(repo("open/open.asl")).unwrap();
    let want: BTreeSet<&str> = [
        "record-padding",
        "idnt-uuid",
        "document-mode",
        "lefx-master-switch",
        "lefx-num-modifying",
        "blend-options-empty",
        "blend-options-order",
        "blend-options-full",
    ]
    .into_iter()
    .collect();
    assert_eq!(rule_ids(&bytes), want, "{:#?}", violations(&bytes));
}

/// `hand/H11.asl`, the form that crashed Photoshop 26.10: a `blendOptions`
/// without `Opct`/`Md  `. Only the full-form rule catches it, and the reader
/// still reads it as before.
#[test]
fn the_h11_form_breaks_only_the_full_blend_options_rule() {
    let bytes = std::fs::read(hand_fixture("H11.asl")).unwrap();
    let want: BTreeSet<&str> = ["blend-options-full"].into_iter().collect();
    assert_eq!(rule_ids(&bytes), want, "{:#?}", violations(&bytes));
    let report = decode(&bytes);
    let styles = report["styles"].as_array().unwrap();
    assert_eq!(styles.len(), 1, "{report}");
    let got = &styles[0];
    assert_eq!(got["name"], json!("H11"));
    assert_eq!(got["dropped"], json!([]));
    assert_eq!(got["style"], canonical(&no_blending_preset()["style"]));
    assert!(got["blending"].is_null(), "{got}");
}

#[test]
fn every_export_keeps_every_rule() {
    let mut cases: Vec<(String, Vec<Value>)> = vec![("R1..R8".into(), take_presets(None))];
    for preset in [
        stacked_preset(),
        blending_preset(),
        no_blending_preset(),
        one_effect_preset(),
        blend_if_only_preset(),
        blending_only_preset(),
    ] {
        cases.push((preset["name"].as_str().unwrap().into(), vec![preset]));
    }
    for (label, presets) in cases {
        let bytes = encode(&Value::Array(presets.clone()));
        assert_eq!(violations(&bytes), vec![], "{label}");
        let report = decode(&bytes);
        let styles = report["styles"].as_array().unwrap();
        assert_eq!(styles.len(), presets.len(), "{label}");
        for (got, want) in styles.iter().zip(&presets) {
            let name = &want["name"];
            assert_eq!(got["name"], *name, "{label}");
            assert_eq!(got["dropped"], json!([]), "{label} {name}");
            assert_eq!(got["style"], canonical(&want["style"]), "{label} {name}");
            let blending = written_blending(want);
            assert_eq!(got["blending"]["mode"], blending["mode"], "{label} {name}");
            let (a, b) = (
                got["blending"]["opacity"].as_f64(),
                blending["opacity"].as_f64(),
            );
            assert_eq!(
                a.map(|v| (v * 1e4).round()),
                b.map(|v| (v * 1e4).round()),
                "{label} {name}"
            );
        }
    }
}

/// The owner's hand-load files: nothing unless `ASL_HAND_DIR` names
/// a folder. Then `<ASL_HAND_TAG>.asl` (default `H1`) holds R1..R8 named
/// `<tag>-R<n>`; with `ASL_HAND_SINGLES=1` also `H<n+2>-R<n>.asl` holding
/// R<n> alone and `H11.asl` holding [`no_blending_preset`] named `H11`.
/// With `ASL_HAND_BLENDING=1` it writes only `H12.asl`
/// ([`no_blending_preset`] named `H12`), `H13.asl`
/// ([`blend_if_only_preset`] named `H13`) and `H14.asl`
/// ([`blending_only_preset`] named `H14`), and no `<tag>.asl`.
#[test]
fn writes_the_hand_load_files_when_asked() {
    let Some(dir) = std::env::var_os("ASL_HAND_DIR").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let write = |file: String, presets: Vec<Value>| {
        let bytes = encode(&Value::Array(presets));
        std::fs::write(dir.join(&file), &bytes).unwrap();
        eprintln!(
            "wrote {file}: {} bytes, {:?}",
            bytes.len(),
            rule_ids(&bytes)
        );
    };
    if std::env::var("ASL_HAND_BLENDING").as_deref() == Ok("1") {
        for (tag, mut preset) in [
            ("H12", no_blending_preset()),
            ("H13", blend_if_only_preset()),
            ("H14", blending_only_preset()),
        ] {
            preset["name"] = json!(tag);
            write(format!("{tag}.asl"), vec![preset]);
        }
        return;
    }
    let tag = std::env::var("ASL_HAND_TAG").unwrap_or_else(|_| "H1".to_string());
    write(format!("{tag}.asl"), take_presets(Some(&format!("{tag}-"))));
    if std::env::var("ASL_HAND_SINGLES").as_deref() != Ok("1") {
        return;
    }
    for (n, preset) in (1..=8).zip(take_presets(None)) {
        let name = format!("H{}-R{n}", n + 2);
        let mut preset = preset;
        preset["name"] = json!(name);
        write(format!("{name}.asl"), vec![preset]);
    }
    let mut h11 = no_blending_preset();
    h11["name"] = json!("H11");
    write("H11.asl".to_string(), vec![h11]);
}
