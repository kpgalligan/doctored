//! Photoshop-AUTHORED style libraries (`.asl`) through `dr_asl_decode`,
//! `dr_asl_encode` and the PSD import FFI.
//!
//! The fixtures (`fixtures/psd_photoshop/styles_asl/`) come from ONE take,
//! `scripts/photoshop-probe/styles_asl_take.py` (Photoshop 26.10.0): S1..S10
//! made as styles from styled layers, each saved with its layered PSD
//! `S<n>.psd`, written to `styles_a.asl` (S1..S5) and `styles_b.asl`
//! (S6..S10); `open/open.asl` is the Doctored-written library the take was
//! to load (R1..R8 exported, R9 = R8 with the white keys swapped). `rows.json`
//! holds what each style was built as. A file of more than 64 KB stays in
//! `~/devel/doctored-fixtures/photoshop/styles-asl-557/take1/fixture` and a
//! test of it skips without it (`take1_557`). The open take is
//! `psd_asl_open_668.rs`: it judges the PSDs Photoshop saved after loading a
//! fresh library from the fixed writer, not `open/open.asl` (whose R9 is
//! retired).
//!
//! The ORACLES are independent of the importer: each style's `expect` in
//! `rows.json` is what the runner told Photoshop to build (numbers exact,
//! fractions within 0.005, colours as exact hex), matched effect by effect
//! by type and instance order; and the layered PSD Photoshop saved from the
//! same layer, read through the PSD importer, must give the same effects.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
use doctored_core::ffi_style::{
    dr_asl_decode, dr_asl_encode, dr_doc_layer_style, dr_style_preset_canonical,
};
use serde_json::{json, Value};

use crate::common::{cpath, take_err_string};

/// A fraction's bound against the number the runner built it from.
pub(crate) const FRACTION: f64 = 0.005;
const KEYS: [&[u8]; 8] = [
    b"SrcB", b"Srcl", b"SrcW", b"Srcm", b"DstB", b"Dstl", b"DstW", b"Dstt",
];

// --------------------------------------------------------------- the files --

fn repo(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/styles_asl")
        .join(name)
}

/// The take's own copy of a fixture file (module doc).
fn take1_557(name: &str) -> Option<PathBuf> {
    let root: PathBuf = std::env::var_os("HOME")?.into();
    let path = root
        .join("devel/doctored-fixtures/photoshop/styles-asl-557/take1/fixture")
        .join(name);
    path.is_file().then_some(path)
}

/// The fixture `name` from the repo, else from the take; `None` (the test
/// skips) when neither holds it.
fn fixture(name: &str) -> Option<PathBuf> {
    let path = repo(name);
    if path.is_file() {
        return Some(path);
    }
    let found = take1_557(name);
    if found.is_none() {
        eprintln!("skipped: {name} is in neither the repo nor the take folder");
    }
    found
}

fn rows() -> Value {
    let text = std::fs::read_to_string(repo("rows.json")).expect("rows.json");
    serde_json::from_str(&text).unwrap()
}

// ------------------------------------------------------------- C plumbing --

fn decode(bytes: &[u8]) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_decode(bytes.as_ptr(), bytes.len(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

fn encode(presets: &Value) -> Vec<u8> {
    let c = CString::new(presets.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_encode(c.as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let out: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    assert_eq!(out["skipped"], json!([]), "{out}");
    base64(out["asl"].as_str().unwrap())
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

fn canonical(style: &Value) -> Value {
    let c = CString::new(style.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_style_preset_canonical(c.as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

/// The style of the top layer (`FS_source`, the styled one) of a
/// Photoshop-saved PSD, canonicalized as a preset; `None` when the PSD
/// importer carries none.
fn psd_style(path: &Path) -> Option<Value> {
    psd_top_layer(path).0
}

/// The top layer of a Photoshop-saved PSD through the PSD importer: its
/// style canonicalized as a preset (`None` when the importer carries none),
/// its blend mode and its opacity.
pub(crate) fn psd_top_layer(path: &Path) -> (Option<Value>, BlendMode, f32) {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    let report: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), 0, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    let document: Box<DrDocument> = unsafe { Box::from_raw(raw) };
    let top = document.layers.len() - 1;
    let layer = &document.layers[top];
    let (blend, opacity) = (layer.blend, layer.opacity);
    let raw = unsafe { dr_doc_layer_style(&*document, top) };
    if raw.is_null() {
        return (None, blend, opacity);
    }
    let style: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    (Some(canonical(&style)), blend, opacity)
}

/// The enabled effects of `style` (a disabled effect draws nothing:
/// Photoshop's PSD record lists every kind, the disabled ones `enab` false,
/// and the PSD importer keeps them as disabled effects).
pub(crate) fn enabled(style: &Value) -> Value {
    Value::Array(
        style["effects"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter(|e| e["enabled"] != false)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
    )
}

/// Whether `key` = `value` on an effect of `kind` is the default the
/// canonical writer leaves out.
fn is_default(kind: &str, key: &str, value: &Value) -> bool {
    let mut effect = json!({"type": kind});
    effect[key] = value.clone();
    let style = canonical(&json!({"effects": [effect]}));
    style["effects"][0].get(key).is_none()
}

// ---------------------------------------------------------------- matching --

/// `want` as a subset of `got` (module doc's tolerances), or the first path
/// where it is not.
fn subset(want: &Value, got: &Value, path: &str) -> Result<(), String> {
    match (want, got) {
        (Value::Object(w), Value::Object(g)) => {
            for (key, value) in w {
                let at = format!("{path}.{key}");
                let found = g.get(key).ok_or_else(|| format!("{at} is missing"))?;
                subset(value, found, &at)?;
            }
            Ok(())
        }
        (Value::Array(w), Value::Array(g)) if w.len() == g.len() => w
            .iter()
            .zip(g)
            .enumerate()
            .try_for_each(|(i, (a, b))| subset(a, b, &format!("{path}[{i}]"))),
        (Value::Number(w), Value::Number(g)) => {
            let (a, b) = (w.as_f64().unwrap(), g.as_f64().unwrap());
            let exact = a.fract() == 0.0;
            let close = if exact {
                a == b
            } else {
                (a - b).abs() <= FRACTION
            };
            close
                .then_some(())
                .ok_or_else(|| format!("{path}: {b} against {a}"))
        }
        _ if want == got => Ok(()),
        _ => Err(format!("{path}: {got} against {want}")),
    }
}

pub(crate) fn types(effects: &Value) -> Vec<String> {
    let mut out: Vec<String> = effects
        .as_array()
        .map(|list| {
            list.iter()
                .map(|e| e["type"].as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Each expected effect matched to the same-typed effect of the same
/// instance order in `got`, and no effect type more or less.
fn effects_match(want: &Value, got: &Value, label: &str) -> Result<(), String> {
    if types(want) != types(got) {
        return Err(format!(
            "{label}: effects {:?} against {:?}",
            types(got),
            types(want)
        ));
    }
    let got = got.as_array().unwrap();
    let mut seen: Vec<String> = Vec::new();
    for w in want.as_array().unwrap() {
        let kind = w["type"].as_str().unwrap().to_string();
        let instance = seen.iter().filter(|k| **k == kind).count();
        seen.push(kind.clone());
        let g = got
            .iter()
            .filter(|e| e["type"] == kind.as_str())
            .nth(instance)
            .unwrap();
        // A key the canonical writer leaves out holds its default.
        let mut w = w.clone();
        if let Value::Object(o) = &mut w {
            o.retain(|key, value| g.get(key).is_some() || !is_default(&kind, key, value));
        }
        subset(&w, g, &format!("{label}.{kind}[{instance}]"))?;
    }
    Ok(())
}

pub(crate) fn strip_anchor(value: &mut Value) {
    match value {
        Value::Object(o) => {
            o.remove("anchor");
            o.values_mut().for_each(strip_anchor);
        }
        Value::Array(a) => a.iter_mut().for_each(strip_anchor),
        _ => {}
    }
}

/// Every style of both Photoshop libraries, by key (`S<n>`).
fn photoshop_styles() -> Option<Vec<(String, Value)>> {
    let rows = rows();
    let run = rows["run_id"].as_str().unwrap().to_string();
    let mut out = Vec::new();
    for file in ["styles_a.asl", "styles_b.asl"] {
        let bytes = std::fs::read(fixture(file)?).unwrap();
        for style in decode(&bytes)["styles"].as_array().unwrap() {
            let name = style["name"].as_str().unwrap();
            let key = name
                .strip_prefix(&format!("{run}-"))
                .unwrap_or_else(|| panic!("{name} is not the take's"));
            out.push((key.to_string(), style.clone()));
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ tests --

#[test]
fn every_photoshop_style_imports_as_it_was_built() {
    let Some(styles) = photoshop_styles() else {
        return;
    };
    let rows = rows();
    let keys: Vec<&str> = styles.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        ["S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10"]
    );
    let mut failures = Vec::new();
    for (key, style) in &styles {
        let expect = &rows["styles"][key]["expect"];
        // The committed rows predate the product rename: a dropped sentence
        // names the product the way the build that wrote them did.
        let renamed: Value = serde_json::from_str(
            &expect["dropped"]
                .to_string()
                .replace("which Rasterize does", "which Doctored does"),
        )
        .unwrap();
        if style["dropped"] != renamed {
            failures.push(format!(
                "{key}: dropped {} against {}",
                style["dropped"], expect["dropped"]
            ));
        }
        if let Err(e) = effects_match(&expect["effects"], &style["style"]["effects"], key) {
            failures.push(e);
        }
        if let Err(e) = subset(&expect["root"], &style["style"], key) {
            failures.push(e);
        }
        if expect["blending"].is_null() {
            if !style["blending"].is_null() {
                failures.push(format!("{key}: blending {}", style["blending"]));
            }
        } else if let Err(e) = subset(&expect["blending"], &style["blending"], key) {
            failures.push(e);
        }
        if style["style"]["blend_if"] != expect["blend_if"] {
            failures.push(format!(
                "{key}: blend_if {} against {}",
                style["style"]["blend_if"], expect["blend_if"]
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// BLND: S9's gray+red Blend If reads back exactly, so Photoshop's key order
/// is `asl::BLND_RAMPS` (the take's verdict, `rows.json` `blnd`).
#[test]
fn the_blnd_keys_read_photoshops_gray_and_red_ranges() {
    let Some(styles) = photoshop_styles() else {
        return;
    };
    let rows = rows();
    assert_eq!(rows["blnd"], "SWAPPED-WHITE");
    let s9 = &styles.iter().find(|(k, _)| k == "S9").unwrap().1;
    assert_eq!(
        s9["style"]["blend_if"],
        json!({"channel":"gray","this_layer":[10,40,200,230],"underlying":[20,60,180,240],
               "extra":[{"channel":"red","this_layer":[5,30,220,250],
                         "underlying":[3,25,235,252]}]})
    );
    assert_eq!(s9["dropped"], json!([]));
}

/// The `Blnd` keys in file order from `at` on.
fn key_order(bytes: &[u8], at: usize) -> Vec<&'static [u8]> {
    let mut out = Vec::new();
    let mut i = at;
    while i + 12 <= bytes.len() {
        let hit = KEYS.iter().find(|k| {
            bytes[i..i + 4] == [0, 0, 0, 0]
                && &bytes[i + 4..i + 8] == **k
                && &bytes[i + 8..i + 12] == b"long"
        });
        if let Some(k) = hit {
            out.push(*k);
            i += 12;
        } else {
            i += 1;
        }
    }
    out
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// The R8/R9 record matching the verdict (R9: the white keys swapped) is
/// the key order `dr_asl_encode` writes for the built R8 style.
#[test]
fn the_export_writes_the_verdicts_key_order() {
    let Some(path) = fixture("open/open.asl") else {
        return;
    };
    let rows = rows();
    let open = std::fs::read(path).unwrap();
    let r9 = rows["presets"]["R9"]["name"].as_str().unwrap();
    let at = open
        .windows(utf16(r9).len())
        .position(|w| w == utf16(r9).as_slice())
        .expect("R9 is in the open library");
    let want = key_order(&open, at);
    assert_eq!(want.len(), 16, "two entries of eight keys");
    let r8 = &rows["presets"]["R8"];
    let written = encode(&json!([{"name": r8["name"], "style": r8["style"],
                                   "blending": r8["blending"]}]));
    assert_eq!(key_order(&written, 0), want);
    // And it reads back as built.
    let back = decode(&written);
    assert_eq!(back["styles"][0]["style"], canonical(&r8["style"]));
}

/// Each Photoshop-saved PSD gives its layer the enabled effects its library
/// gives the style made from that layer. A style with an effect Doctored
/// cannot render (S10's Stroke Emboss) is the one difference by design: the
/// `.asl` reader drops that effect and names it, the PSD importer declines
/// the record's whole style (`psd_import`'s verified loss).
#[test]
fn each_psd_carries_its_librarys_effects() {
    let Some(styles) = photoshop_styles() else {
        return;
    };
    let mut failures = Vec::new();
    for (key, style) in &styles {
        let Some(path) = fixture(&format!("{key}.psd")) else {
            continue;
        };
        let from_psd = psd_style(&path);
        if style["dropped"] != json!([]) {
            if from_psd.is_some() {
                failures.push(format!("{key}: the PSD importer carried a declined style"));
            }
            continue;
        }
        let Some(from_psd) = from_psd else {
            failures.push(format!("{key}: the PSD carries no style"));
            continue;
        };
        let mut from_psd = enabled(&from_psd);
        let mut from_asl = enabled(&style["style"]);
        strip_anchor(&mut from_psd);
        strip_anchor(&mut from_asl);
        if from_psd != from_asl {
            failures.push(format!("{key}: PSD {from_psd} against .asl {from_asl}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every Photoshop library reimports unchanged through the writer.
#[test]
fn each_photoshop_library_round_trips_through_the_writer() {
    for file in ["styles_a.asl", "styles_b.asl"] {
        let Some(path) = fixture(file) else {
            continue;
        };
        let first = decode(&std::fs::read(path).unwrap());
        let presets: Vec<Value> = first["styles"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| !s["style"].is_null())
            .map(|s| json!({"name": s["name"], "style": s["style"], "blending": s["blending"]}))
            .collect();
        let again = decode(&encode(&Value::Array(presets.clone())));
        for (a, b) in again["styles"].as_array().unwrap().iter().zip(&presets) {
            assert_eq!(a["name"], b["name"], "{file}");
            assert_eq!(a["style"], b["style"], "{file} {}", a["name"]);
            assert_eq!(a["blending"], b["blending"], "{file} {}", a["name"]);
            assert_eq!(a["dropped"], json!([]), "{file} {}", a["name"]);
        }
        assert_eq!(again["styles"].as_array().unwrap().len(), presets.len());
    }
}
