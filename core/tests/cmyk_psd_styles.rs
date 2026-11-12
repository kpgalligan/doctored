//! Layer styles and the nine adjustment kinds in a layered CMYK PSD (#1,
//! the styles batch): Doctored's own export reopened exactly (every enabled
//! effect with its inks, Fill, knockout, Blend Interior, the nine adjustment
//! records' parameters, the Photo Filter ink exactly through colour space 2);
//! Photoshop take1's files (`photoshop-probe/cmyk_styles_take.py`) read
//! against the parameters the take DECLARED when it authored them — effect
//! and range parameters exact, a colour that reached ink through `to_ink`
//! within 2 ink points a plate — skipping when the take is absent; a Blend
//! If in a CMYK file carried with its declared ranges; and the Layers-mode refusals (an
//! adjustment kind with no Photoshop record, a style Photoshop cannot hold)
//! equal between the export report and the save, with Flattened unaffected.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_psd::{dr_precision_doc_psd_export_report, dr_precision_doc_save_psd};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_psd_import::{open_psd, report as import_report};
use crate::cmyk_structure_fixture::{free, LIMIT};
use crate::cmyk_style_fixture::{backdrop, hard_rect, layer, percent, soft_rect, styled};
use crate::common::{cpath, take_err_string};

const W: u32 = 24;
const H: u32 = 20;
const PSD: i32 = 0;
const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;

fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn add(doc: *mut PrecisionDocument, name: &str, op: &str, params: Value) -> *mut PrecisionDocument {
    let description = json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params});
    let name = CString::new(name).unwrap();
    let text = CString::new(description.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            text.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "add {op}: {}", take_err_string(err));
    free(doc);
    out
}

fn report(doc: *const PrecisionDocument, mode: i32) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_psd_export_report(doc, PSD, mode, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn save(doc: *const PrecisionDocument, path: &Path, mode: i32) -> Result<(), String> {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            PSD,
            mode,
            LIMIT,
            0,
            &mut err,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

fn style_of(doc: *const PrecisionDocument, name: &str) -> Value {
    let reported = unsafe { &*doc }
        .layer(index(doc, name))
        .unwrap()
        .reported_style();
    reported.map_or(Value::Null, |s| serde_json::from_str(&s).unwrap())
}

/// A style's ENABLED effects and its blending options: what a PSD carries.
fn carried(style: &Value) -> Value {
    let effects: Vec<&Value> = style["effects"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter(|e| e["enabled"] != json!(false))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "effects": effects,
        "fill_opacity": style.get("fill_opacity"),
        "knockout": style.get("knockout"),
        "blend_interior_as_group": style.get("blend_interior_as_group"),
    })
}

fn description(doc: *const PrecisionDocument, name: &str) -> Value {
    let doc_ref = unsafe { &*doc };
    serde_json::from_str(doc_ref.adjustment_description(index(doc, name)).unwrap()).unwrap()
}

/// The nine kinds at parameters on Photoshop's grids, named by kind.
fn nine() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "Bright",
            "bcs",
            json!({"brightness":0.2,"contrast":0.3,"legacy":false}),
        ),
        (
            "Balance",
            "color_balance",
            json!({"shadows":{"cyan_red":0.1,"magenta_green":-0.05,"yellow_blue":0.0},
            "midtones":{"cyan_red":-0.1,"magenta_green":0.15,"yellow_blue":0.05},
            "highlights":{"cyan_red":0.0,"magenta_green":0.0,"yellow_blue":-0.2},"preserve_luminosity":true}),
        ),
        (
            "Selective",
            "selective_color",
            json!({"method":"absolute",
            "reds":{"c":0.1,"m":-0.1,"y":0.05,"k":0.0},"blacks":{"c":0.0,"m":0.0,"y":0.0,"k":0.1}}),
        ),
        (
            "Mixer",
            "channel_mixer",
            json!({"monochrome":false,
            "cyan":{"c":0.9,"m":0.1,"y":0.0,"k":0.0,"constant":0.05},"magenta":{"c":0.0,"m":1.0,"y":0.0,"k":0.0,"constant":0.0},
            "yellow":{"c":0.0,"m":0.0,"y":1.0,"k":0.0,"constant":-0.05},"black":{"c":0.0,"m":0.0,"y":0.1,"k":0.9,"constant":0.0}}),
        ),
        (
            "Filter",
            "photo_filter",
            json!({"ink":[0.1,0.4,0.9,0.3],"density":0.6,"preserve_luminosity":true}),
        ),
        ("Invert", "invert", json!({})),
        ("Posterize", "posterize", json!({"levels":5})),
        (
            "Threshold",
            "threshold",
            json!({"level":0.5019607843137255}),
        ),
        (
            "Map",
            "gradient_map",
            // Classic at Smoothness 1: interpolation 4096, Photoshop's
            // own `adjust9.psd` body.
            json!({"dither":false,"method":"classic","gradient":{"reverse":false,"stops":[
            {"position":0.0,"ink":[0.0,0.0,0.0,1.0],"opacity":1.0},
            {"position":0.5,"ink":[0.5,0.0,1.0,0.0],"opacity":1.0},
            {"position":1.0,"ink":[0.0,0.0,0.0,0.0],"opacity":1.0}]}}),
        ),
    ]
}

/// A 16-bit CMYK document with a styled raster (every effect with inks), a
/// knockout layer, a Blend Interior layer in an isolated group, and the nine
/// adjustment layers.
fn styled_document() -> *mut PrecisionDocument {
    let ink = |p: [f64; 4]| json!(percent(p));
    let e1 = ink([75.0, 68.0, 67.0, 90.0]);
    let e2 = ink([100.0, 0.0, 0.0, 0.0]);
    let doc = backdrop(
        W,
        H,
        16,
        &[[0.0; 4], [50.0, 0.0, 0.0, 0.0], [20.0, 40.0, 60.0, 10.0]],
    );
    let doc = layer(
        doc,
        "Shape",
        [10.0, 60.0, 30.0, 0.0],
        &soft_rect(W, H, (6, 5, 18, 15)),
    );
    let style = json!({"fill_opacity":0.6,"effects":[
        {"type":"drop_shadow","ink":e1,"blend":"multiply","opacity":0.75,"angle":120,"use_global_light":false,"distance":4,"size":3,"spread":0.2},
        {"type":"inner_shadow","ink":e2,"opacity":0.5,"angle":45,"use_global_light":false,"distance":2,"size":2},
        {"type":"outer_glow","ink":e2,"blend":"screen","opacity":0.6,"size":4},
        {"type":"inner_glow","ink":e1,"opacity":0.4,"size":3},
        {"type":"bevel_emboss","highlight_ink":e2,"shadow_ink":e1,"size":3,"angle":120,"altitude":30,"use_global_light":false},
        {"type":"satin","ink":e1,"opacity":0.5,"angle":19,"distance":6,"size":5},
        {"type":"color_overlay","ink":e2,"opacity":0.3},
        {"type":"gradient_overlay","opacity":0.5,"gradient":{"stops":[
            {"position":0.0,"ink":e1,"opacity":1.0},{"position":1.0,"ink":e2,"opacity":1.0}]}},
        {"type":"stroke","ink":e1,"size":2,"position":"outside"}]});
    let doc = styled(doc, index(doc, "Shape"), &style);
    let doc = layer(
        doc,
        "Knock",
        [0.0, 0.0, 100.0, 0.0],
        &hard_rect(W, H, (2, 2, 10, 10)),
    );
    let doc = styled(
        doc,
        index(doc, "Knock"),
        &json!({"knockout":"deep","fill_opacity":0.0}),
    );
    let doc = layer(
        doc,
        "Inner",
        [0.0, 50.0, 0.0, 0.0],
        &hard_rect(W, H, (12, 8, 22, 18)),
    );
    let doc = styled(
        doc,
        index(doc, "Inner"),
        &json!({"blend_interior_as_group":true,"effects":[{"type":"color_overlay","ink":e1,"blend":"multiply","opacity":0.5}]}),
    );
    let mut doc = doc;
    for (name, op, params) in nine() {
        doc = add(doc, name, op, params);
    }
    doc
}

#[test]
fn a_styled_cmyk_document_reopens_from_its_own_psd_exactly() {
    let dir = TempDir::new().unwrap();
    let doc = styled_document();
    let r = report(doc, LAYERS);
    assert_eq!(r["available"], true, "{}", r["refusal"]);
    let path = dir.path().join("styled.psd");
    save(doc, &path, LAYERS).unwrap();
    let back = open_psd(&path, 0).unwrap();
    for name in ["Shape", "Knock", "Inner"] {
        assert_eq!(
            carried(&style_of(back, name)),
            carried(&style_of(doc, name)),
            "{name}"
        );
    }
    for (name, op, params) in nine() {
        let got = description(back, name);
        assert_eq!(got["op"], json!(op), "{name}");
        assert_eq!(got["params"], description(doc, name)["params"], "{name}");
        // The parameters given are on Photoshop's grids, so stored as given.
        holds(&params, &got["params"], name);
    }
    free(back);
    free(doc);
}

/// `got` holds every key of `declared` at its value: numbers within `1e-4`,
/// an `effects` list matched by type among the enabled effects.
fn holds(declared: &Value, got: &Value, at: &str) {
    match declared {
        Value::Object(map) => {
            for (key, want) in map {
                let here = format!("{at}.{key}");
                if key == "effects" {
                    for effect in want.as_array().unwrap() {
                        let found = got["effects"]
                            .as_array()
                            .and_then(|list| {
                                list.iter().find(|e| {
                                    e["type"] == effect["type"] && e["enabled"] != json!(false)
                                })
                            })
                            .unwrap_or_else(|| panic!("{here}: no enabled {}", effect["type"]));
                        holds(effect, found, &here);
                    }
                } else {
                    holds(want, &got[key], &here);
                }
            }
        }
        Value::Array(list) => {
            let other = got
                .as_array()
                .unwrap_or_else(|| panic!("{at}: {got} is not a list"));
            assert_eq!(list.len(), other.len(), "{at}");
            for (i, (a, b)) in list.iter().zip(other).enumerate() {
                holds(a, b, &format!("{at}[{i}]"));
            }
        }
        Value::Number(n) => {
            let g = got
                .as_f64()
                .unwrap_or_else(|| panic!("{at}: {got} is not a number"));
            assert!((n.as_f64().unwrap() - g).abs() <= 1e-4, "{at}: {g} vs {n}");
        }
        other => assert_eq!(got, other, "{at}"),
    }
}

fn take1() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir =
        PathBuf::from(home).join("devel/doctored-fixtures/photoshop/cmyk-styles-547/take1/psd");
    dir.join("manifest.json").is_file().then_some(dir)
}

#[test]
fn photoshops_styled_cmyk_files_read_as_the_take_declared() {
    let Some(dir) = take1() else {
        eprintln!("skipping: cmyk-styles-547/take1 is absent");
        return;
    };
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    for (case, entry) in manifest["cases"].as_object().unwrap() {
        let path = dir.join(entry["file"].as_str().unwrap());
        let doc = open_psd(&path, 0).unwrap_or_else(|e| panic!("{case}: {e}"));
        let name = entry["layer"].as_str().unwrap();
        let got = style_of(doc, name);
        let layer = unsafe { &*doc }.layer(index(doc, name)).unwrap().clone();
        let declared = &entry["style"];
        // The Blend If batch: the declared ranges carry exactly, and nothing
        // about them is reported lost.
        let ranges = &declared["blend_if"];
        match layer.blend_if() {
            None => assert!(ranges.is_null(), "{case}: {ranges} was not carried"),
            Some(b) => {
                assert_eq!(
                    json!({"channel": b.channel.display_name().to_lowercase(),
                           "this_layer": b.this_layer, "underlying": b.underlying}),
                    *ranges,
                    "{case}"
                );
                let losses = &import_report(&path)["verified_losses"];
                assert!(
                    !losses
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|l| l.as_str().unwrap().contains("Blend If")),
                    "{case}: {losses}"
                );
            }
        }
        if case.starts_with("bi_") {
            // Its ranges are checked above; it has no other style.
        } else if case.ends_with("_default") {
            // D-K (take1 FITTED): Photoshop's default black is K100.
            let effect = &declared["effects"][0];
            let found = got["effects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["type"] == effect["type"] && e["enabled"] != json!(false))
                .unwrap();
            let ink = if effect["type"] == "bevel_emboss" {
                &found["shadow_ink"]
            } else {
                &found["ink"]
            };
            assert_eq!(ink, &json!([0.0, 0.0, 0.0, 1.0]), "{case}");
            holds(effect, found, case);
        } else if !declared.is_null() {
            holds(declared, &got, case);
        }
        free(doc);
    }
    // The nine adjustment kinds (`adjust9`, `cmyk_files_author`), against
    // the parameters the take declared in Photoshop's units.
    let doc = open_psd(&dir.join("adjust9.psd"), 0).unwrap();
    for spec in manifest["adjust9"]["layers"].as_array().unwrap() {
        let name = spec["name"].as_str().unwrap();
        let got = &description(doc, name)["params"];
        let p = &spec["params"];
        let unit = |v: &Value| v.as_f64().unwrap() / 100.0;
        let want = match spec["kind"].as_str().unwrap() {
            "brightness_contrast" => {
                json!({"brightness": p["brightness"].as_f64().unwrap() / 150.0,
                "contrast": unit(&p["contrast"]), "legacy": p["legacy"]})
            }
            "color_balance" => {
                let tones = |v: &Value| {
                    let v = v.as_array().unwrap();
                    json!({"cyan_red":unit(&v[0]),"magenta_green":unit(&v[1]),"yellow_blue":unit(&v[2])})
                };
                json!({"shadows":tones(&p["shadows"]),"midtones":tones(&p["midtones"]),
                    "highlights":tones(&p["highlights"]),"preserve_luminosity":p["preserve_luminosity"]})
            }
            "selective_color" => {
                let cmyk = |v: &Value| {
                    let v = v.as_array().unwrap();
                    json!({"c":unit(&v[0]),"m":unit(&v[1]),"y":unit(&v[2]),"k":unit(&v[3])})
                };
                json!({"method":p["method"],"reds":cmyk(&p["reds"]),"neutrals":cmyk(&p["neutrals"]),"blacks":cmyk(&p["blacks"])})
            }
            "channel_mixer_cmyk" => {
                let row = |v: &Value| {
                    let v = v.as_array().unwrap();
                    json!({"c":unit(&v[0]),"m":unit(&v[1]),"y":unit(&v[2]),"k":unit(&v[3]),"constant":unit(&v[4])})
                };
                if p["monochrome"] == json!(true) {
                    json!({"monochrome":true,"gray":row(&p["cyan"])})
                } else {
                    json!({"monochrome":false,"cyan":row(&p["cyan"]),"magenta":row(&p["magenta"]),
                        "yellow":row(&p["yellow"]),"black":row(&p["black"])})
                }
            }
            "photo_filter_cmyk" => {
                // The ink reached the file as Photoshop's Lab: 2 ink points.
                let declared: Vec<f64> = p["ink"].as_array().unwrap().iter().map(unit).collect();
                let ink = got["ink"].as_array().unwrap();
                for c in 0..4 {
                    assert!(
                        (ink[c].as_f64().unwrap() - declared[c]).abs() <= 0.02,
                        "{name}: {ink:?}"
                    );
                }
                json!({"density":unit(&p["density"]),"preserve_luminosity":p["preserve_luminosity"]})
            }
            "invert" => json!({}),
            "posterize" => json!({"levels":p["levels"]}),
            "threshold" => json!({"level": p["level"].as_f64().unwrap() / 255.0}),
            "gradient_map_cmyk" => {
                let stops: Vec<Value> = p["stops"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| {
                        let s = s.as_array().unwrap();
                        json!({"position": s[4].as_f64().unwrap() / 4096.0,
                        "ink":[unit(&s[0]),unit(&s[1]),unit(&s[2]),unit(&s[3])],"opacity":1.0})
                    })
                    .collect();
                // Photoshop's interpolation word 4096: Classic.
                json!({"dither":p["dither"],"method":"classic","gradient":{"stops":stops}})
            }
            other => panic!("{name}: kind {other}"),
        };
        holds(&want, got, name);
    }
    free(doc);
}

/// The body of the first `grdm` block in a saved file.
fn grdm_body(bytes: &[u8]) -> Vec<u8> {
    let at = bytes
        .windows(8)
        .position(|w| w == b"8BIMgrdm")
        .expect("a grdm block");
    let length = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
    bytes[at + 12..at + 12 + length].to_vec()
}

/// One CMYK document holding a single Gradient Map layer, saved Layers; its
/// `grdm` body.
fn saved_grdm(params: Value) -> Vec<u8> {
    let dir = TempDir::new().unwrap();
    let doc = add(
        backdrop(W, H, 16, &[[0.0; 4]]),
        "Map",
        "gradient_map",
        params,
    );
    let path = dir.path().join("map.psd");
    save(doc, &path, LAYERS).unwrap();
    free(doc);
    grdm_body(&std::fs::read(path).unwrap())
}

/// Adobe's "Gradient settings" layout, field by field, as (name, bytes): the
/// oracle both `grdm` tests read a body through.
fn grdm_fields(body: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut at = 0;
    let mut out = Vec::new();
    let mut take = |name: String, count: usize, out: &mut Vec<(String, Vec<u8>)>| {
        let field = body[at..at + count].to_vec();
        at += count;
        out.push((name, field.clone()));
        field
    };
    let u16_of = |b: &[u8]| usize::from(u16::from_be_bytes([b[0], b[1]]));
    take("version".into(), 2, &mut out);
    take("reversed, dithered".into(), 2, &mut out);
    take("method".into(), 4, &mut out);
    let units = take("name length".into(), 4, &mut out);
    let units = u32::from_be_bytes(units.try_into().unwrap()) as usize;
    take("name".into(), 2 * units, &mut out);
    let count = u16_of(&take("colour stops".into(), 2, &mut out));
    for i in 0..count {
        take(format!("colour stop {i}"), 20, &mut out);
    }
    let count = u16_of(&take("transparency stops".into(), 2, &mut out));
    for i in 0..count {
        take(format!("transparency stop {i}"), 10, &mut out);
    }
    for (name, count) in [
        ("expansion, interpolation, length, mode", 8),
        ("seed", 4),
        ("show transparency, vector colour", 4),
        ("roughness", 4),
        ("colour model", 2),
        ("minimum, maximum colour", 16),
        ("dummy", 2),
    ] {
        take(name.into(), count, &mut out);
    }
    out.push(("pad".into(), body[at..].to_vec()));
    out
}

/// Photoshop refused a whole layered CMYK PSD/PSB, and read its TIFF
/// flat, for a `grdm` body ending two bytes short of a multiple of 4. A
/// two-stop K100 → paper map is Adobe's 130-byte layout, two zero bytes of
/// pad, with Photoshop's own two opaque transparency stops and seed (a
/// Classic map at Smoothness 1, so its interpolation word is 4096).
#[test]
fn a_gradient_map_record_is_adobes_layout_padded_to_a_multiple_of_four() {
    let body = saved_grdm(
        json!({"dither":false,"method":"classic","gradient":{"reverse":false,"stops":[
        {"position":0.0,"ink":[0.0,0.0,0.0,1.0],"opacity":1.0},
        {"position":1.0,"ink":[0.0,0.0,0.0,0.0],"opacity":1.0}]}}),
    );
    let mut want = Vec::new();
    let u16s = |want: &mut Vec<u8>, values: &[u16]| {
        for v in values {
            want.extend_from_slice(&v.to_be_bytes());
        }
    };
    let u32s = |want: &mut Vec<u8>, values: &[u32]| {
        for v in values {
            want.extend_from_slice(&v.to_be_bytes());
        }
    };
    u16s(&mut want, &[3]); // version 3
    want.extend_from_slice(&[0, 0]); // not reversed, not dithered
    want.extend_from_slice(b"Gcls"); // Classic interpolation
    u32s(&mut want, &[7]); // "Custom" + NUL, UTF-16
    u16s(&mut want, &[0x43, 0x75, 0x73, 0x74, 0x6f, 0x6d, 0]);
    u16s(&mut want, &[2]); // colour stops: location, midpoint, space 2, 65535·(1 − ink), user
    u32s(&mut want, &[0, 50]);
    u16s(&mut want, &[2, 0xffff, 0xffff, 0xffff, 0, 0]);
    u32s(&mut want, &[4096, 50]);
    u16s(&mut want, &[2, 0xffff, 0xffff, 0xffff, 0xffff, 0]);
    u16s(&mut want, &[2]); // transparency stops: location, midpoint, opacity
    u32s(&mut want, &[0, 50]);
    u16s(&mut want, &[255]);
    u32s(&mut want, &[4096, 50]);
    u16s(&mut want, &[255]);
    u16s(&mut want, &[2, 4096, 32, 0]); // expansion, interpolation, length, mode
    u32s(&mut want, &[0x4428_5f2b]); // seed (Photoshop's own)
    u16s(&mut want, &[0, 0]); // show transparency, vector colour
    u32s(&mut want, &[2048]); // roughness
    u16s(&mut want, &[3]); // colour model
    u16s(&mut want, &[0, 0, 0, 0, 0x8000, 0x8000, 0x8000, 0x8000]); // min, max colour
    u16s(&mut want, &[0]); // dummy
    assert_eq!(want.len(), 130, "Adobe's layout for two stops");
    want.extend_from_slice(&[0, 0]); // pad to a multiple of 4
    assert_eq!(grdm_fields(&body), grdm_fields(&want));
    assert_eq!(body, want);
    // A map with a translucent stop keeps one transparency stop per colour
    // stop, and is padded the same way.
    let body = saved_grdm(json!({"dither":false,"gradient":{"reverse":false,"stops":[
        {"position":0.0,"ink":[0.0,0.0,0.0,1.0],"opacity":1.0},
        {"position":0.5,"ink":[0.5,0.0,1.0,0.0],"opacity":0.4},
        {"position":1.0,"ink":[0.0,0.0,0.0,0.0],"opacity":1.0}]}}));
    let fields = grdm_fields(&body);
    let count = fields
        .iter()
        .find(|(n, _)| n == "transparency stops")
        .unwrap();
    assert_eq!(count.1, vec![0, 3]);
    assert_eq!(body.len() % 4, 0, "{}", body.len());
}

/// The same Gradient Map Photoshop wrote in its own CMYK file (styles take1's
/// `adjust9.psd`: K100 → C50 Y100 → paper) is written field for field — and
/// so byte for byte — as Photoshop's record, its 2-byte pad included.
#[test]
fn a_gradient_map_record_matches_photoshops_own_field_for_field() {
    let Some(dir) = take1() else {
        eprintln!("skipping: cmyk-styles-547/take1 is absent");
        return;
    };
    let theirs = grdm_body(&std::fs::read(dir.join("adjust9.psd")).unwrap());
    let map = nine()
        .into_iter()
        .find(|(name, _, _)| *name == "Map")
        .unwrap();
    let ours = saved_grdm(map.2);
    assert_eq!(theirs.len(), 152);
    assert_eq!(grdm_fields(&ours), grdm_fields(&theirs));
    assert_eq!(ours, theirs);
}

/// One CMYK document holding a single Gradient Map layer of `params`, saved
/// Layers at `path`.
fn map_psd(params: Value, path: &Path) {
    let doc = add(
        backdrop(W, H, 16, &[[0.0; 4]]),
        "Map",
        "gradient_map",
        params,
    );
    save(doc, path, LAYERS).unwrap();
    free(doc);
}

/// The file offset of the `grdm` body's interpolation word (the u16 after
/// the expansion count), read through `grdm_fields`.
fn interpolation_offset(file: &[u8]) -> usize {
    let start = file
        .windows(8)
        .position(|w| w == b"8BIMgrdm")
        .expect("a grdm block")
        + 12;
    let body = grdm_body(file);
    let fields = grdm_fields(&body);
    let before: usize = fields
        .iter()
        .take_while(|(n, _)| n != "expansion, interpolation, length, mode")
        .map(|(_, b)| b.len())
        .sum();
    start + before + 2
}

/// The reopened Map layer's params.
fn reopened_map(path: &Path) -> Value {
    let back = open_psd(path, 0).unwrap();
    let params = description(back, "Map")["params"].clone();
    free(back);
    params
}

const LEGACY_MAP: &str = r#"{"dither":false,"gradient":{"reverse":false,"stops":[
    {"position":0.0,"ink":[0.0,0.0,0.0,1.0],"opacity":1.0},
    {"position":0.5,"ink":[0.5,0.0,1.0,0.0],"opacity":1.0},
    {"position":1.0,"ink":[0.0,0.0,0.0,0.0],"opacity":1.0}]}}"#;

/// A legacy map (no `method`) writes interpolation 0 and every `Mdpn`
/// 50, so it reopens legacy; the body an EARLIER build wrote for the same
/// map (interpolation 4096) reopens as `method: "classic"` at Smoothness 1,
/// as Photoshop renders it — the deliberate re-read change.
#[test]
fn a_legacy_map_writes_interpolation_0_and_an_earlier_builds_body_reads_classic() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("legacy.psd");
    map_psd(serde_json::from_str(LEGACY_MAP).unwrap(), &path);
    let mut file = std::fs::read(&path).unwrap();
    let at = interpolation_offset(&file);
    assert_eq!(&file[at..at + 2], &[0, 0], "a legacy map is linear");
    let legacy = reopened_map(&path);
    assert_eq!(legacy.get("method"), None, "{legacy}");
    assert_eq!(legacy["gradient"].get("smoothness"), None, "{legacy}");
    // What an earlier build wrote: the same body with Photoshop's 4096.
    file[at..at + 2].copy_from_slice(&4096u16.to_be_bytes());
    let old = dir.path().join("earlier-build.psd");
    std::fs::write(&old, &file).unwrap();
    let classic = reopened_map(&old);
    assert_eq!(classic["method"], json!("classic"), "{classic}");
    assert_eq!(classic["gradient"].get("smoothness"), None, "{classic}");
    assert_eq!(classic["gradient"]["stops"], legacy["gradient"]["stops"]);
}

/// A Classic map's Smoothness and midpoints survive the `grdm` round
/// trip, the midpoint shift running in POSITION order: stored unsorted as
/// `[B@1, A@0 midpoint 0.3]`, Photoshop stop 1 (B) carries A's 30, and the
/// map reopens with 0.3 on A..B.
#[test]
fn an_unsorted_classic_map_round_trips_its_midpoints_in_position_order() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("unsorted.psd");
    map_psd(
        json!({"dither":false,"method":"classic","gradient":{"smoothness":0.5,"stops":[
            {"position":1.0,"ink":[0.0,0.0,0.0,0.0],"midpoint":0.5},
            {"position":0.0,"ink":[0.0,0.0,0.0,1.0],"midpoint":0.3}]}}),
        &path,
    );
    let file = std::fs::read(&path).unwrap();
    let fields = grdm_fields(&grdm_body(&file));
    let stop = |i: usize| {
        let (_, b) = fields
            .iter()
            .find(|(n, _)| *n == format!("colour stop {i}"))
            .unwrap();
        (
            u32::from_be_bytes(b[0..4].try_into().unwrap()),
            u32::from_be_bytes(b[4..8].try_into().unwrap()),
        )
    };
    assert_eq!(
        stop(0),
        (0, 50),
        "Photoshop stop 0: A, its Mdpn governs nothing"
    );
    assert_eq!(
        stop(1),
        (4096, 30),
        "Photoshop stop 1: B carries A's midpoint"
    );
    let at = interpolation_offset(&file);
    assert_eq!(u16::from_be_bytes([file[at], file[at + 1]]), 2048);
    let back = reopened_map(&path);
    assert_eq!(back["method"], json!("classic"), "{back}");
    assert_eq!(back["gradient"]["smoothness"], json!(0.5), "{back}");
    let stops = back["gradient"]["stops"].as_array().unwrap();
    assert_eq!(stops[0]["position"], json!(0.0));
    assert_eq!(stops[0]["midpoint"], json!(0.3), "{back}");
    assert_eq!(stops[1].get("midpoint"), None, "{back}");
}

#[test]
fn what_photoshop_cannot_hold_refuses_the_layers_mode_alike_in_report_and_save() {
    let dir = TempDir::new().unwrap();
    let lut = crate::float_adjustments::base64(
        &(0..8u32)
            .flat_map(|i| [(i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32])
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<u8>>(),
    );
    let base = || {
        layer(
            backdrop(W, H, 16, &[[0.0; 4]]),
            "Shape",
            [10.0, 60.0, 30.0, 0.0],
            &hard_rect(W, H, (4, 4, 16, 16)),
        )
    };
    let lookup = add(
        base(),
        "Lookup",
        "color_lookup",
        json!({"kind":"3d","size":2,"table":lut}),
    );
    let thin = styled(
        base(),
        1,
        &json!({"effects":[{"type":"stroke","size":0.5,"ink":[0.0,0.0,0.0,1.0]}]}),
    );
    for (doc, sentence) in [
        (lookup, "Adjustment layer “Lookup” (color_lookup) has no Photoshop record in a CMYK file; export Flattened"),
        (thin, "“Shape”'s layer style needs baking ("),
    ] {
        let r = report(doc, LAYERS);
        assert_eq!(r["available"], false);
        let refusal = r["refusal"].as_str().unwrap().to_string();
        assert!(refusal.starts_with(sentence), "{refusal}");
        let path = dir.path().join("refused.psd");
        assert_eq!(save(doc, &path, LAYERS).unwrap_err(), refusal);
        assert!(!path.exists(), "nothing is written");
        assert_eq!(report(doc, FLATTENED)["available"], true);
        save(doc, &dir.path().join("flat.psd"), FLATTENED).unwrap();
        free(doc);
    }
}

/// D13 in a CMYK file: the CMYK writer stores no Photoshop Background
/// record and the CMYK import sets no Background mark, so a Deep knockout
/// over a plain, opaque, canvas-sized bottom is disclosed by the export
/// report and again by the import report of the file it wrote.
#[test]
fn a_deep_knockout_over_a_plain_bottom_is_disclosed_on_export_and_import() {
    const EXPORT: &str = "A layer uses knockout over this document's bottom layer, which Doctored treats as the Background. A PSD written by Doctored stores it as an ordinary layer, and Photoshop punches a Deep knockout (or a Shallow one outside every group) through to transparency when a file has no Background layer, so the hole can show transparency in Photoshop where it shows the bottom layer here.";
    const IMPORT: &str = "This file has no Photoshop Background layer, and a knockout can reach its bottom layer, which Doctored treats as the Background because it is visible, Normal, 100 % opacity, unmasked, unstyled, canvas-sized and at the origin. Photoshop punches a Deep knockout (or a Shallow one outside every group) through to transparency when a file has no Background layer, so the hole can show the bottom layer here where it shows transparency in Photoshop. This is a deliberate Doctored rule, not an import error.";
    let has = |list: &Value, sentence: &str| {
        list.as_array()
            .unwrap()
            .iter()
            .any(|line| line.as_str() == Some(sentence))
    };
    let dir = TempDir::new().unwrap();
    let doc = layer(
        backdrop(W, H, 16, &[[0.0; 4]]),
        "Knock",
        [0.0, 0.0, 100.0, 0.0],
        &hard_rect(W, H, (2, 2, 10, 10)),
    );
    let doc = styled(doc, index(doc, "Knock"), &json!({"knockout":"deep"}));
    let exported = report(doc, LAYERS);
    assert!(has(&exported["losses"], EXPORT), "{exported}");
    let path = dir.path().join("knock.psd");
    save(doc, &path, LAYERS).unwrap();
    let imported = import_report(&path);
    assert!(has(&imported["verified_losses"], IMPORT), "{imported}");
    free(doc);
}

#[test]
fn a_cmyk_files_pattern_overlay_is_carried_with_its_pattern() {
    let dir = TempDir::new().unwrap();
    let doc = layer(
        backdrop(W, H, 16, &[[0.0; 4]]),
        "Shape",
        [10.0, 60.0, 30.0, 0.0],
        &hard_rect(W, H, (4, 4, 16, 16)),
    );
    let effect = json!({"type":"pattern_overlay","pattern":{"preset":"stripes"},
                        "scale":0.5,"phase":[3,1]});
    let doc = styled(doc, 1, &json!({ "effects": [effect] }));
    // The CMYK file carries the effect and an RGB `Pat2` record for its
    // pattern (`psd_patterns`); the reopened record reads both back.
    let path = dir.path().join("pattern.psd");
    save(doc, &path, LAYERS).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        bytes.windows(8).any(|w| w == b"8BIMPat2"),
        "the pattern table"
    );
    let back = open_psd(&path, 0).unwrap();
    let carried = style_of(back, "Shape");
    let effects = carried["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1, "{carried}");
    assert_eq!(effects[0]["type"], "pattern_overlay");
    assert_eq!(effects[0]["pattern"], json!({"preset":"stripes"}));
    assert_eq!(effects[0]["scale"], 0.5);
    assert_eq!(effects[0]["phase"], json!([3.0, 1.0]));
    assert_eq!(style_of(back, "Shape"), style_of(doc, "Shape"));
    free(back);
    free(doc);
}
