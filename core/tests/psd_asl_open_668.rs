//! Session O: Photoshop loads and applies Doctored's `.asl` as written.
//!
//! The library is a fresh `dr_asl_encode` export of `rows.json`'s R1..R8
//! (their tile renamed `<build id>-tile`) plus the hand-2 presets H12, H13
//! and H14 (`psd_asl_layout`'s no-blending, Blend-If-only and blending-only
//! presets), written by `writes_the_open_library_when_asked` for
//! `scripts/photoshop-probe/styles_asl_take.py --build-668` (module doc
//! section SESSION O). Photoshop loads it, applies each preset to a
//! fresh layer and saves `<key>.psd`; each PSD is read back through
//! Doctored's own PSD importer.
//!
//! The ORACLE is what each preset declared (`declared`): its canonical style
//! and the layer blending the writer gives it. The BOUNDS (`misses`), fixed
//! before any session: the enabled effect types equal; each declared effect
//! against the read effect of the same type and instance order with `same`
//! (strings, booleans and null exact, so colours are exact hex; a number
//! whole on both sides exact; any other number within `FRACTION`); the root
//! `fill_opacity` (default 1) within `FRACTION`, `knockout` (absent = none),
//! `blend_interior_as_group` (false) and `blend_if` (null) exact; the
//! layer's mode exact and its opacity within `FRACTION`.
//!
//! `open668/readings.json` is the judged take: every key's reading and its
//! misses (a key with none is absent). `the_committed_readings_hold_the_declared_presets`
//! recomputes the misses from the readings, so only drift fails; the take
//! test reads the take's PSDs again when they are on disk and skips when not.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use doctored_core::doc::BlendMode;
use serde_json::{json, Map, Value};

use crate::asl_layout::violations;
use crate::psd_asl_layout::{
    blend_if_only_preset, blending_only_preset, canonical, decode, encode, no_blending_preset,
    take_presets, written_blending,
};
use crate::psd_asl_photoshop::{enabled, psd_top_layer, strip_anchor, types, FRACTION};

const KEYS: [&str; 11] = [
    "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "H12", "H13", "H14",
];

/// The build id `the_open_library_keeps_every_rule` writes the library under
/// (the runner's selftest id, the same length as every real one).
const FIXED_ID: &str = "rz-557-aslb-20260101T000000Z-00000000";

// --------------------------------------------------------------- the files --

fn repo(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/styles_asl")
        .join(name)
}

/// An environment path; a relative one is the repo root's (the runner's
/// commands run from there, the test binary from `core/`).
fn env_path(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    if path.is_absolute() {
        return Some(path);
    }
    Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path))
}

fn rows_build_id() -> String {
    let text = std::fs::read_to_string(repo("rows.json")).expect("rows.json");
    let rows: Value = serde_json::from_str(&text).unwrap();
    rows["build_id"].as_str().unwrap().to_string()
}

/// The 11 presets of the library under `build_id`, by key (module doc):
/// R1..R8 named `<id>-R<n>` with the rows' tile renamed `<id>-tile`, then
/// H12, H13 and H14 named `<id>-H1x`.
fn open_presets(build_id: &str) -> Vec<(String, Value)> {
    let (old, new) = (
        format!("{}-tile", rows_build_id()),
        format!("{build_id}-tile"),
    );
    let mut out = Vec::new();
    for (n, mut preset) in (1..=8).zip(take_presets(Some(&format!("{build_id}-")))) {
        let text = preset["style"].to_string().replace(&old, &new);
        preset["style"] = serde_json::from_str(&text).unwrap();
        out.push((format!("R{n}"), preset));
    }
    for (key, mut preset) in [
        ("H12", no_blending_preset()),
        ("H13", blend_if_only_preset()),
        ("H14", blending_only_preset()),
    ] {
        preset["name"] = json!(format!("{build_id}-{key}"));
        out.push((key.to_string(), preset));
    }
    out
}

/// `presets` encoded: no layout violation, and every preset reads back with
/// its name, its canonical style and its written blending.
fn checked_library(presets: &[(String, Value)]) -> Vec<u8> {
    let list = Value::Array(presets.iter().map(|(_, p)| p.clone()).collect());
    let bytes = encode(&list);
    assert_eq!(violations(&bytes), vec![]);
    let report = decode(&bytes);
    let styles = report["styles"].as_array().unwrap();
    assert_eq!(styles.len(), presets.len(), "{report}");
    for (got, (key, want)) in styles.iter().zip(presets) {
        assert_eq!(got["name"], want["name"], "{key}");
        assert_eq!(got["dropped"], json!([]), "{key}");
        assert_eq!(got["style"], canonical(&want["style"]), "{key}");
        let blending = written_blending(want);
        assert_eq!(got["blending"]["mode"], blending["mode"], "{key}");
        let (a, b) = (
            got["blending"]["opacity"].as_f64(),
            blending["opacity"].as_f64(),
        );
        assert_eq!(
            a.map(|v| (v * 1e4).round()),
            b.map(|v| (v * 1e4).round()),
            "{key}"
        );
    }
    bytes
}

// ---------------------------------------------------------------- matching --

/// What `preset` declares (module doc): its canonical style, anchors
/// stripped, and the layer blending the writer gives it (Normal 100 % when
/// it writes none).
fn declared(preset: &Value) -> Value {
    let mut style = canonical(&preset["style"]);
    strip_anchor(&mut style);
    let blending = written_blending(preset);
    json!({"style": style, "layer": {
        "mode": blending["mode"].as_str().unwrap_or("normal"),
        "opacity": blending["opacity"].as_f64().unwrap_or(1.0)}})
}

/// The top layer of a Photoshop-saved PSD through the PSD importer: its
/// canonical style, anchors stripped (null when it carries none), its mode
/// and opacity.
fn reading(path: &Path) -> Value {
    let (style, blend, opacity) = psd_top_layer(path);
    let mut style = style.unwrap_or(Value::Null);
    strip_anchor(&mut style);
    let mode = match blend {
        BlendMode::Normal => "normal".to_string(),
        BlendMode::Multiply => "multiply".to_string(),
        other => format!("other:{other:?}"),
    };
    json!({"style": style, "layer": {"mode": mode, "opacity": f64::from(opacity)}})
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= FRACTION
}

/// `want` and `got` the same under the module doc's bounds, in both
/// directions (the same key sets), or the first path where they are not.
/// The two-way twin of `psd_asl_photoshop::subset`.
fn same(want: &Value, got: &Value, path: &str) -> Result<(), String> {
    match (want, got) {
        (Value::Object(w), Value::Object(g)) => {
            let (wk, gk): (BTreeSet<&String>, BTreeSet<&String>) =
                (w.keys().collect(), g.keys().collect());
            if wk != gk {
                let read: Vec<_> = gk.difference(&wk).collect();
                let unread: Vec<_> = wk.difference(&gk).collect();
                return Err(format!("{path}: keys {read:?} read, {unread:?} not read"));
            }
            w.iter()
                .try_for_each(|(key, value)| same(value, &g[key], &format!("{path}.{key}")))
        }
        (Value::Array(w), Value::Array(g)) => {
            if w.len() != g.len() {
                return Err(format!("{path}: {} items against {}", g.len(), w.len()));
            }
            w.iter()
                .zip(g)
                .enumerate()
                .try_for_each(|(i, (a, b))| same(a, b, &format!("{path}[{i}]")))
        }
        (Value::Number(w), Value::Number(g)) => {
            let (a, b) = (w.as_f64().unwrap(), g.as_f64().unwrap());
            let ok = if a.fract() == 0.0 && b.fract() == 0.0 {
                a == b
            } else {
                close(a, b)
            };
            ok.then_some(())
                .ok_or_else(|| format!("{path}: {b} against {a}"))
        }
        _ if want == got => Ok(()),
        _ => Err(format!("{path}: {got} against {want}")),
    }
}

/// A root field of `style`, its default when absent or null.
fn root(style: &Value, field: &str, default: Value) -> Value {
    style
        .get(field)
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or(default)
}

/// Every way `reading` breaks the module doc's bounds against `declared`,
/// one line each.
fn misses(declared: &Value, reading: &Value, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (want, got) = (enabled(&declared["style"]), enabled(&reading["style"]));
    if types(&want) != types(&got) {
        out.push(format!(
            "{key}: effects {:?} against {:?}",
            types(&got),
            types(&want)
        ));
    }
    let got_list = got.as_array().unwrap();
    let mut seen: Vec<String> = Vec::new();
    for w in want.as_array().unwrap() {
        let kind = w["type"].as_str().unwrap_or("").to_string();
        let instance = seen.iter().filter(|k| **k == kind).count();
        seen.push(kind.clone());
        let found = got_list
            .iter()
            .filter(|e| e["type"] == kind.as_str())
            .nth(instance);
        if let Some(g) = found {
            if let Err(e) = same(w, g, &format!("{key}.{kind}[{instance}]")) {
                out.push(e);
            }
        }
    }
    let (ws, gs) = (&declared["style"], &reading["style"]);
    let (a, b) = (
        root(ws, "fill_opacity", json!(1.0)),
        root(gs, "fill_opacity", json!(1.0)),
    );
    if !close(
        a.as_f64().unwrap_or(f64::NAN),
        b.as_f64().unwrap_or(f64::NAN),
    ) {
        out.push(format!("{key}: fill_opacity {b} against {a}"));
    }
    for (field, default) in [
        ("knockout", json!("none")),
        ("blend_interior_as_group", json!(false)),
        ("blend_if", Value::Null),
    ] {
        let (a, b) = (root(ws, field, default.clone()), root(gs, field, default));
        if a != b {
            out.push(format!("{key}: {field} {b} against {a}"));
        }
    }
    let (wl, gl) = (&declared["layer"], &reading["layer"]);
    if wl["mode"] != gl["mode"] {
        out.push(format!(
            "{key}: layer mode {} against {}",
            gl["mode"], wl["mode"]
        ));
    }
    let (a, b) = (wl["opacity"].as_f64(), gl["opacity"].as_f64());
    if !close(a.unwrap_or(f64::NAN), b.unwrap_or(f64::NAN)) {
        out.push(format!(
            "{key}: layer opacity {} against {}",
            gl["opacity"], wl["opacity"]
        ));
    }
    out
}

fn committed() -> Option<Value> {
    let path = repo("open668/readings.json");
    if !path.is_file() {
        eprintln!("skipped: {} is not committed", path.display());
        return None;
    }
    Some(serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap())
}

// ------------------------------------------------------------------ tests --

#[test]
fn the_open_library_keeps_every_rule() {
    let presets = open_presets(FIXED_ID);
    let keys: Vec<&str> = presets.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, KEYS);
    let text = Value::Array(presets.iter().map(|(_, p)| p.clone()).collect()).to_string();
    assert!(
        !text.contains(&rows_build_id()),
        "the rows' build id survives"
    );
    checked_library(&presets);
}

/// The runner's library (`--build-668`): nothing unless `ASL_OPEN_DIR` and
/// `ASL_OPEN_ID` are set; then `<id>.asl` and `presets.json` in that folder.
#[test]
fn writes_the_open_library_when_asked() {
    let (Some(dir), Ok(id)) = (env_path("ASL_OPEN_DIR"), std::env::var("ASL_OPEN_ID")) else {
        return;
    };
    let presets = open_presets(&id);
    let bytes = checked_library(&presets);
    std::fs::create_dir_all(&dir).unwrap();
    let open_file = format!("{id}.asl");
    std::fs::write(dir.join(&open_file), &bytes).unwrap();
    let mut table = Map::new();
    for (key, preset) in &presets {
        table.insert(
            key.clone(),
            json!({"name": preset["name"], "style": preset["style"],
                   "blending": preset["blending"],
                   "written_blending": written_blending(preset)}),
        );
    }
    let record = json!({"build_id": id, "open_file": open_file, "keys": KEYS,
                        "presets": table});
    let text = serde_json::to_string_pretty(&record).unwrap() + "\n";
    std::fs::write(dir.join("presets.json"), text).unwrap();
    eprintln!("wrote {open_file}: {} bytes", bytes.len());
}

#[test]
fn the_committed_readings_hold_the_declared_presets() {
    let Some(profile) = committed() else {
        return;
    };
    let build_id = profile["build_id"].as_str().unwrap();
    let readings = profile["readings"].as_object().unwrap();
    let held: BTreeSet<&str> = readings.keys().map(String::as_str).collect();
    assert_eq!(held, KEYS.into_iter().collect::<BTreeSet<_>>());
    for (key, preset) in open_presets(build_id) {
        let want: Vec<String> = profile["misses"]
            .get(&key)
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .map(|l| l.as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            misses(&declared(&preset), &readings[&key], &key),
            want,
            "{key}"
        );
    }
}

/// The take's own PSDs (`<take>/fixture/open668/<key>.psd`; `ASL_OPEN_TAKE`,
/// else `styles-asl-668/take1`) read as the committed readings; skips when a
/// PSD is absent. With `ASL_OPEN_READINGS_OUT` it writes the profile there
/// instead (the take's `build.json` names the build).
#[test]
fn the_take_psds_read_as_the_committed_readings() {
    let take = env_path("ASL_OPEN_TAKE").or_else(|| {
        let home: PathBuf = std::env::var_os("HOME")?.into();
        Some(home.join("devel/doctored-fixtures/photoshop/styles-asl-668/take1"))
    });
    let Some(take) = take else {
        return;
    };
    let folder = take.join("fixture/open668");
    let mut readings = Map::new();
    for key in KEYS {
        let path = folder.join(format!("{key}.psd"));
        if !path.is_file() {
            eprintln!("skipped: {} is absent", path.display());
            return;
        }
        readings.insert(key.to_string(), reading(&path));
    }
    if let Some(out) = env_path("ASL_OPEN_READINGS_OUT") {
        let text = std::fs::read_to_string(take.join("build.json")).expect("build.json");
        let build: Value = serde_json::from_str(&text).unwrap();
        let build_id = build["build_id"].as_str().unwrap();
        let mut missed = Map::new();
        for (key, preset) in open_presets(build_id) {
            let lines = misses(&declared(&preset), &readings[&key], &key);
            if !lines.is_empty() {
                missed.insert(key, json!(lines));
            }
        }
        let name = take.file_name().unwrap().to_string_lossy().to_string();
        let profile = json!({"build_id": build_id, "take": name, "readings": readings,
                             "misses": missed});
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let text = serde_json::to_string_pretty(&profile).unwrap() + "\n";
        std::fs::write(&out, text).unwrap();
        eprintln!("wrote {}: misses {}", out.display(), json!(missed));
        return;
    }
    let Some(profile) = committed() else {
        return;
    };
    assert_eq!(profile["readings"], Value::Object(readings));
}
