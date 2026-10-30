//! Photoshop adjustment PRESET files through
//! `dr_adjustment_preset_file_decode`: `.acv` Curves reframed onto the `curv`
//! record, `.alv`/`.ahu`/`.eap`/`.blw`/`.cha` as the `levl`/`hue2`/`expA`/
//! `blwh`/`mixr` bodies they are, every refusal sentence, the 64 KiB cap and
//! the NULL sweep.
//!
//! Every file here is an independent byte construction — no Adobe file is
//! committed — and the `.blw` descriptor body comes from the shared
//! hand-encoded builder, never the crate's writer. The one `#[ignore]` probe
//! reads the presets an installed Photoshop ships, and only those.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::ffi_adjustment_preset::dr_adjustment_preset_file_decode;
use serde_json::{json, Map, Value};

use crate::common;
use crate::psd_descriptor_fixture as builder;
use builder::{body, boolean, long, rgbc, text};
use common::take_err_string;

// ------------------------------------------------------------- C plumbing --

/// The decoded `{op, params}`, or the core's message.
fn decode(extension: &str, bytes: &[u8]) -> Result<(String, Map<String, Value>), String> {
    let ext = CString::new(extension).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe {
        dr_adjustment_preset_file_decode(bytes.as_ptr(), bytes.len(), ext.as_ptr(), &mut err)
    };
    if raw.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null(), "success must not set err_out");
    let value: Value = serde_json::from_str(&take_err_string(raw)).expect("decode JSON");
    let op = value["op"].as_str().expect("op").to_string();
    let params = value["params"].as_object().expect("params").clone();
    Ok((op, params))
}

fn refusal(extension: &str, bytes: &[u8]) -> String {
    match decode(extension, bytes) {
        Ok(decoded) => panic!("{extension} decoded unexpectedly: {decoded:?}"),
        Err(message) => message,
    }
}

fn close(value: &Value, expected: f64) {
    let actual = value
        .as_f64()
        .unwrap_or_else(|| panic!("{value} is not a number"));
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

// --------------------------------------------------------------- builders --

fn be16(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_be_bytes());
}

/// An `.acv` file: version, count, then each curve's `(output, input)` pairs.
fn acv(version: u16, curves: &[&[(u16, u16)]]) -> Vec<u8> {
    acv_counted(version, curves.len() as u16, curves)
}

/// An `.acv` whose declared count may disagree with the curves written.
fn acv_counted(version: u16, count: u16, curves: &[&[(u16, u16)]]) -> Vec<u8> {
    let mut out = Vec::new();
    be16(&mut out, version);
    be16(&mut out, count);
    for curve in curves {
        be16(&mut out, curve.len() as u16);
        for (output, input) in *curve {
            be16(&mut out, *output);
            be16(&mut out, *input);
        }
    }
    out
}

const IDENTITY: &[(u16, u16)] = &[(0, 0), (255, 255)];

/// A `.alv` file: version 2, 29 records of (in floor, in ceil, out floor,
/// out ceil, gamma × 100), then `tail`.
fn alv(records: &[[u16; 5]], tail: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    be16(&mut out, 2);
    for index in 0..29 {
        let record = records.get(index).copied().unwrap_or([0, 255, 0, 255, 100]);
        for field in record {
            be16(&mut out, field);
        }
    }
    out.extend_from_slice(tail);
    out
}

/// Photoshop's default Hue/Saturation band ranges (a, b, c, d).
const RANGES: [[i16; 4]; 6] = [
    [315, 345, 15, 45],
    [15, 45, 75, 105],
    [75, 105, 135, 165],
    [135, 165, 195, 225],
    [195, 225, 255, 285],
    [255, 285, 315, 345],
];

/// A `.ahu` file: version 2, the colorize flag, the colorization and master
/// triples, then six untouched bands.
fn ahu(colorize: bool, tint: [i16; 3], master: [i16; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    be16(&mut out, 2);
    out.push(u8::from(colorize));
    out.push(0);
    for value in tint.into_iter().chain(master) {
        out.extend(value.to_be_bytes());
    }
    for range in RANGES {
        for value in range.into_iter().chain([0, 0, 0]) {
            out.extend(value.to_be_bytes());
        }
    }
    out
}

/// A `.eap` file: version 1 then three f32s, with or without the 2-byte pad.
fn eap(exposure: f32, offset: f32, gamma: f32, padded: bool) -> Vec<u8> {
    let mut out = Vec::new();
    be16(&mut out, 1);
    for value in [exposure, offset, gamma] {
        out.extend(value.to_be_bytes());
    }
    if padded {
        out.extend([0, 0]);
    }
    out
}

/// A `.blw` file: the `blwh` descriptor body in Photoshop's item order.
fn blw(weights: [i32; 6], use_tint: u8, tint: [f64; 3]) -> Vec<u8> {
    body(
        16,
        b"null",
        &[
            long(b"Rd  ", weights[0]),
            long(b"Yllw", weights[1]),
            long(b"Grn ", weights[2]),
            long(b"Cyn ", weights[3]),
            long(b"Bl  ", weights[4]),
            long(b"Mgnt", weights[5]),
            boolean(b"useTint", use_tint),
            rgbc(b"tintColor", tint[0], tint[1], tint[2]),
            long(b"bwPresetKind", 1),
            text(b"blackAndWhitePresetFileName", ""),
        ],
    )
}

// ------------------------------------------------------------------ .acv --

#[test]
fn curves_v4_with_an_identity_fifth_curve_maps_the_first_four_channels() {
    let mut file = acv(
        4,
        &[
            &[(0, 0), (150, 128), (255, 255)],
            &[(10, 0), (255, 255)],
            IDENTITY,
            &[(0, 20), (240, 255)],
            IDENTITY,
        ],
    );
    // Bytes after the last curve are a tail, ignored.
    file.extend(b"Crv \x00\x01");
    let (op, params) = decode("acv", &file).expect("v4 decodes");
    assert_eq!(op, "curves");
    let expected = json!({
        "rgb": [[0, 0], [128, 150], [255, 255]],
        "r": [[0, 10], [255, 255]],
        "g": [[0, 0], [255, 255]],
        "b": [[20, 0], [255, 240]],
    });
    assert_eq!(Value::Object(params), expected);
}

#[test]
fn curves_v1_with_one_curve_is_the_composite_only() {
    let file = acv(1, &[&[(0, 0), (90, 64), (200, 192), (255, 255)]]);
    let (op, params) = decode("acv", &file).expect("v1 decodes");
    assert_eq!(op, "curves");
    assert_eq!(
        Value::Object(params),
        json!({"rgb": [[0, 0], [64, 90], [192, 200], [255, 255]]})
    );
}

#[test]
fn curves_refusals_name_the_reason() {
    let cmyk = acv(
        1,
        &[
            IDENTITY,
            IDENTITY,
            IDENTITY,
            IDENTITY,
            &[(30, 0), (255, 255)],
        ],
    );
    assert_eq!(
        refusal("acv", &cmyk),
        "curves preset curve 5 is not identity; only RGB presets (at most four curves) are \
         supported"
    );
    let three_point_fifth = acv(
        4,
        &[
            IDENTITY,
            IDENTITY,
            IDENTITY,
            IDENTITY,
            &[(0, 0), (128, 128), (255, 255)],
        ],
    );
    assert!(refusal("acv", &three_point_fifth).starts_with("curves preset curve 5 is not"));
    let truncated = acv_counted(4, 2, &[IDENTITY]);
    assert_eq!(refusal("acv", &truncated), "curves preset is truncated");
    let mut cut = acv(1, &[IDENTITY]);
    cut.pop();
    assert_eq!(refusal("acv", &cut), "curves preset is truncated");
    assert_eq!(refusal("acv", &[0, 1]), "curves preset is truncated");
    assert_eq!(
        refusal("acv", &acv(2, &[IDENTITY])),
        "curves preset version 2 is not 1 or 4"
    );
    assert_eq!(
        refusal("acv", &acv_counted(1, 0, &[])),
        "curves preset holds 0 curves; expected 1 to 32"
    );
    assert_eq!(
        refusal("acv", &acv_counted(1, 33, &[IDENTITY; 33])),
        "curves preset holds 33 curves; expected 1 to 32"
    );
    // A curve the record codec refuses is refused with its sentence.
    let one_point = acv(1, &[&[(0, 0)]]);
    assert_eq!(
        refusal("acv", &one_point),
        "this Photoshop Curves preset cannot be used: its point count is outside 2…16"
    );
}

// ------------------------------------------------------------------ .alv --

#[test]
fn levels_composite_only_preset_with_a_tail_decodes() {
    let file = alv(&[[10, 240, 0, 255, 120]], b"Lvls\x00\x00\x00\x03");
    let (op, params) = decode("alv", &file).expect("alv decodes");
    assert_eq!(op, "levels");
    assert_eq!(params.len(), 3, "{params:?}");
    close(&params["black"], 10.0 / 255.0);
    close(&params["white"], 240.0 / 255.0);
    close(&params["gamma"], 1.2);
}

/// A per-channel record and an output range carry. The master
/// keeps its three keys plus the output pair; only the channel that is not
/// the identity record appears, with its input keys and no output pair.
#[test]
fn levels_per_channel_and_output_preset_carries() {
    let file = alv(
        &[
            [0, 255, 20, 235, 100],
            [20, 255, 0, 255, 100],
            [0, 255, 0, 255, 100],
            [0, 200, 0, 255, 80],
        ],
        b"",
    );
    let (op, params) = decode("alv", &file).expect("alv decodes");
    assert_eq!(op, "levels");
    let mut keys: Vec<&str> = params.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "b",
            "black",
            "gamma",
            "output_black",
            "output_white",
            "r",
            "white"
        ],
        "{params:?}"
    );
    close(&params["black"], 0.0);
    close(&params["white"], 1.0);
    close(&params["gamma"], 1.0);
    close(&params["output_black"], 20.0 / 255.0);
    close(&params["output_white"], 235.0 / 255.0);
    let red = params["r"].as_object().expect("r object");
    assert_eq!(red.len(), 3, "{red:?}");
    close(&red["black"], 20.0 / 255.0);
    close(&red["white"], 1.0);
    close(&red["gamma"], 1.0);
    let blue = params["b"].as_object().expect("b object");
    assert_eq!(blue.len(), 3, "{blue:?}");
    close(&blue["black"], 0.0);
    close(&blue["white"], 200.0 / 255.0);
    close(&blue["gamma"], 0.8);
}

// ------------------------------------------------------------------ .ahu --

#[test]
fn hue_saturation_colorize_preset_decodes_its_colorize_keys() {
    let file = ahu(true, [200, 40, -10], [0, 0, 0]);
    let (op, params) = decode("ahu", &file).expect("ahu decodes");
    assert_eq!(op, "hue_saturation");
    assert_eq!(params["colorize"], json!(true));
    close(&params["colorize_hue"], 200.0);
    close(&params["colorize_saturation"], 0.4);
    close(&params["colorize_lightness"], -0.1);
    close(&params["hue"], 0.0);
    // Reds straddle 0°: b = 345, c = 15 → centre 0, inner 15, falloff 30.
    let reds = &params["bands"]["reds"];
    close(&reds["center"], 0.0);
    close(&reds["inner"], 15.0);
    close(&reds["falloff"], 30.0);
}

// ------------------------------------------------------------------ .eap --

#[test]
fn exposure_preset_decodes_without_an_evaluation_at_both_lengths() {
    for padded in [false, true] {
        let file = eap(1.5, -0.0625, 0.75, padded);
        assert_eq!(file.len(), if padded { 16 } else { 14 });
        let (op, params) = decode("eap", &file).expect("eap decodes");
        assert_eq!(op, "exposure");
        let keys: Vec<&str> = params.keys().map(String::as_str).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, ["exposure", "gamma", "offset"], "{params:?}");
        close(&params["exposure"], 1.5);
        close(&params["offset"], -0.0625);
        close(&params["gamma"], 0.75);
    }
}

// ------------------------------------------------------------------ .blw --

#[test]
fn black_and_white_preset_decodes_weights_tint_and_its_colour_verbatim() {
    let file = blw([40, 60, 40, 60, 20, 80], 1, [225.0, 211.0, 179.0]);
    let (op, params) = decode("blw", &file).expect("blw decodes");
    assert_eq!(op, "black_and_white");
    for (key, weight) in [
        ("reds", 0.4),
        ("yellows", 0.6),
        ("greens", 0.4),
        ("cyans", 0.6),
        ("blues", 0.2),
        ("magentas", 0.8),
    ] {
        close(&params[key], weight);
    }
    assert_eq!(params["tint"], json!(true));
    assert_eq!(params["tint_color"], json!("#e1d3b3"));
}

// ------------------------------------------------------------------ .cha --

/// A `.cha` file: the 44-byte `mixr` body — version, monochrome, then four
/// `[R, G, B, K, constant]` percent records, big-endian `i16`.
fn cha(version: i16, monochrome: i16, records: [[i16; 5]; 4]) -> Vec<u8> {
    [version, monochrome]
        .into_iter()
        .chain(records.into_iter().flatten())
        .flat_map(i16::to_be_bytes)
        .collect()
}

const NO_RECORD: [i16; 5] = [0; 5];

#[test]
fn channel_mixer_preset_with_monochrome_decodes_its_gray_row() {
    let file = cha(
        1,
        1,
        [[-55, 130, 25, 0, 4], NO_RECORD, NO_RECORD, NO_RECORD],
    );
    assert_eq!(file.len(), 44);
    let (op, params) = decode("cha", &file).expect("cha decodes");
    assert_eq!(op, "channel_mixer");
    assert_eq!(
        Value::Object(params),
        json!({"monochrome": true, "gray": {"r": -0.55, "g": 1.3, "b": 0.25, "constant": 0.04}})
    );
}

#[test]
fn channel_mixer_preset_without_monochrome_decodes_its_three_rows() {
    let file = cha(
        1,
        0,
        [
            [90, 15, -5, 0, 2],
            [-10, 120, -10, 0, 0],
            [5, -25, 115, 0, -3],
            NO_RECORD,
        ],
    );
    let (op, params) = decode("CHA", &file).expect("upper-case cha decodes");
    assert_eq!(op, "channel_mixer");
    assert_eq!(
        Value::Object(params),
        json!({
            "monochrome": false,
            "red": {"r": 0.9, "g": 0.15, "b": -0.05, "constant": 0.02},
            "green": {"r": -0.1, "g": 1.2, "b": -0.1, "constant": 0.0},
            "blue": {"r": 0.05, "g": -0.25, "b": 1.15, "constant": -0.03},
        })
    );
}

#[test]
fn cmyk_and_short_channel_mixer_presets_are_refused() {
    // A CMYK preset: four output records, each with a K source.
    let cmyk = cha(
        1,
        0,
        [
            [100, 0, 0, 10, 0],
            [0, 100, 0, 10, 0],
            [0, 0, 100, 10, 0],
            [0, 0, 0, 100, 0],
        ],
    );
    assert_eq!(
        refusal("cha", &cmyk),
        "this Photoshop Channel Mixer preset cannot be used: its body holds CMYK channel settings"
    );
    let mut short = cha(1, 1, [[40, 40, 20, 0, 0], NO_RECORD, NO_RECORD, NO_RECORD]);
    short.truncate(40);
    assert_eq!(
        refusal("cha", &short),
        "this Photoshop Channel Mixer preset cannot be used: its body is shorter than its layout"
    );
}

// ------------------------------------------------------- extension and cap --

#[test]
fn extension_is_case_insensitive_and_unknown_ones_are_refused() {
    let file = acv(1, &[&[(0, 0), (255, 255)]]);
    assert_eq!(decode("ACV", &file).expect("upper case").0, "curves");
    assert_eq!(
        refusal("xyz", &file),
        "unsupported preset extension \"xyz\"; expected acv, alv, ahu, eap, blw or cha"
    );
    assert!(refusal(".acv", &file).starts_with("unsupported preset extension"));
    assert!(refusal("", &file).starts_with("unsupported preset extension"));
}

#[test]
fn a_file_over_64_kib_is_refused_before_it_is_read() {
    let mut file = acv(1, &[IDENTITY]);
    file.resize(65_537, 0);
    assert_eq!(
        refusal("acv", &file),
        "preset file is 65537 bytes; the limit is 65536"
    );
    // At the cap exactly, the tail is ignored and the file decodes.
    file.truncate(65_536);
    assert!(decode("acv", &file).is_ok());
    // The cap is checked BEFORE the bytes are touched: a NULL pointer with an
    // oversized length still answers the size sentence.
    let ext = CString::new("acv").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw =
        unsafe { dr_adjustment_preset_file_decode(ptr::null(), 1 << 40, ext.as_ptr(), &mut err) };
    assert!(raw.is_null());
    assert_eq!(
        take_err_string(err),
        format!("preset file is {} bytes; the limit is 65536", 1u64 << 40)
    );
}

// -------------------------------------------------------------- NULL sweep --

#[test]
fn null_arguments_are_tolerated() {
    let ext = CString::new("acv").unwrap();
    let file = acv(1, &[IDENTITY]);
    let mut err: *mut c_char = ptr::null_mut();
    // NULL extension.
    let raw = unsafe {
        dr_adjustment_preset_file_decode(file.as_ptr(), file.len(), ptr::null(), &mut err)
    };
    assert!(raw.is_null());
    assert_eq!(take_err_string(err), "extension is NULL");
    // NULL bytes with a non-zero length.
    err = ptr::null_mut();
    let raw = unsafe { dr_adjustment_preset_file_decode(ptr::null(), 4, ext.as_ptr(), &mut err) };
    assert!(raw.is_null());
    assert_eq!(take_err_string(err), "bytes is NULL");
    // NULL bytes with zero length is an EMPTY file: the format's own
    // truncation sentence, not a null-argument one.
    err = ptr::null_mut();
    let raw = unsafe { dr_adjustment_preset_file_decode(ptr::null(), 0, ext.as_ptr(), &mut err) };
    assert!(raw.is_null());
    assert_eq!(take_err_string(err), "curves preset is truncated");
    let levels = CString::new("alv").unwrap();
    err = ptr::null_mut();
    let raw =
        unsafe { dr_adjustment_preset_file_decode(ptr::null(), 0, levels.as_ptr(), &mut err) };
    assert!(raw.is_null());
    assert_eq!(
        take_err_string(err),
        "this Photoshop Levels preset cannot be used: its body is shorter than its layout"
    );
    // NULL err_out, on failure and on success.
    let raw =
        unsafe { dr_adjustment_preset_file_decode(ptr::null(), 0, ptr::null(), ptr::null_mut()) };
    assert!(raw.is_null());
    let raw = unsafe {
        dr_adjustment_preset_file_decode(file.as_ptr(), file.len(), ext.as_ptr(), ptr::null_mut())
    };
    assert!(!raw.is_null());
    let _ = take_err_string(raw);
}

// ------------------------------------------------------ installed presets --

/// Every preset an installed Photoshop ships in the six folders decodes.
/// Reads only `/Applications/Adobe Photoshop */Presets/<kind>` — never the
/// user's own library.
#[test]
#[ignore = "reads an installed Adobe Photoshop's shipped presets"]
fn installed_photoshop_presets_decode() {
    const FOLDERS: [&str; 6] = [
        "Curves",
        "Levels",
        "Hue and Saturation",
        "Exposure",
        "Black and White",
        "Channel Mixer",
    ];
    let mut decoded = 0;
    let mut failures = Vec::new();
    let apps = std::fs::read_dir("/Applications").expect("/Applications");
    for app in apps.flatten() {
        let name = app.file_name().to_string_lossy().into_owned();
        if !name.starts_with("Adobe Photoshop ") {
            continue;
        }
        for folder in FOLDERS {
            let dir = app.path().join("Presets").join(folder);
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                    continue;
                };
                if !["acv", "alv", "ahu", "eap", "blw", "cha"]
                    .contains(&ext.to_ascii_lowercase().as_str())
                {
                    continue;
                }
                let bytes = std::fs::read(&path).expect("preset bytes");
                match decode(ext, &bytes) {
                    Ok((op, params)) => {
                        assert!(!params.contains_key("evaluation"));
                        println!("{} -> {op} {}", display(&path), Value::Object(params));
                        decoded += 1;
                    }
                    Err(message) => failures.push(format!("{}: {message}", display(&path))),
                }
            }
        }
    }
    println!("decoded {decoded} installed presets");
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(decoded > 0, "no installed Photoshop presets found");
}

fn display(path: &Path) -> String {
    path.display().to_string()
}
