//! #1 Duotone phase 1b (`duotone_ado`, `duotone_files`, the PSD open and
//! save, through the C API): a colour-mode-8 PSD and PSB open as Duotone
//! documents, an unchanged one writes its colour-mode data and duotone
//! resources back byte for byte, an edited one re-encodes, and a Duotone
//! Options (`.ado`) file reads and writes. Since phase 2b, Photoshop's own
//! save of a Tritone PSD Doctored wrote (`duotone3-curves.psd`, the take's
//! case `three`) reads back with the inks Photoshop held and writes its data
//! back byte for byte.
//!
//! Oracles written here, never the crate's writer: the duotone
//! specification's layout as Photoshop 26.10.0 wrote it in the #1 1b
//! smokes (524 bytes: version, count, four 10-byte colour records, four
//! 64-byte Pascal names, four 28-byte curves of 13 tenths-of-a-percent
//! shorts and a 0, the dot gain 20, eleven overprint records; a blank record
//! is space 65535), Adobe's PSD framing (header, colour-mode data, image
//! resources, one gray layer and a Raw composite, built byte by byte), the
//! default overprint W ⊙ Π(Pᵢ ⊘ W) from the CIE Lab formulas, and the
//! Photoshop fixtures `tests/fixtures/psd_photoshop/duotone/` (that half
//! SKIPS with an eprintln when the folder is absent). Quantization, fixed
//! before measuring: Lab 0.01, curve points 0.1 %.

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_duotone::*;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_gray_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::duotone_document::{free, gray_doc, line, set, LIMIT};

const W: u32 = 6;
const H: u32 = 2;
const GRAY: [u8; 12] = [0, 40, 80, 120, 160, 200, 220, 230, 240, 250, 254, 255];

// ---- the specification, as Photoshop lays it out ---------------------------

struct Ink {
    name: &'static str,
    /// The colour record: space and four components.
    record: (u16, [u16; 4]),
    /// Tenths of a percent, -1 blank.
    curve: [i16; 13],
}

fn lab_record(lab: [f64; 3]) -> (u16, [u16; 4]) {
    (
        7,
        [
            (lab[0] * 100.0).round() as u16,
            (lab[1] * 100.0).round() as i16 as u16,
            (lab[2] * 100.0).round() as i16 as u16,
            0,
        ],
    )
}

fn straight(low: i16, high: i16) -> [i16; 13] {
    let mut curve = [-1; 13];
    curve[0] = low;
    curve[12] = high;
    curve
}

fn warm_black() -> Ink {
    Ink {
        name: "Warm Black",
        record: lab_record([20.0, 2.0, 4.0]),
        curve: straight(0, 1000),
    }
}

fn rust(curve: [i16; 13]) -> Ink {
    Ink {
        name: "Rust",
        record: lab_record([55.0, 45.0, 55.0]),
        curve,
    }
}

/// A 524-byte duotone specification of `inks` and `overprints` (records; a
/// missing one is blank).
fn spec(inks: &[Ink], overprints: &[(u16, [u16; 4])]) -> Vec<u8> {
    let mut out = vec![0u8; 524];
    let put =
        |out: &mut Vec<u8>, at: usize, v: u16| out[at..at + 2].copy_from_slice(&v.to_be_bytes());
    put(&mut out, 0, 1);
    put(&mut out, 2, inks.len() as u16);
    for slot in 0..4 {
        let (space, c) = inks.get(slot).map_or((0xffff, [0; 4]), |ink| ink.record);
        put(&mut out, 4 + 10 * slot, space);
        for (i, v) in c.iter().enumerate() {
            put(&mut out, 6 + 10 * slot + 2 * i, *v);
        }
        if let Some(ink) = inks.get(slot) {
            out[44 + 64 * slot] = ink.name.len() as u8;
            out[45 + 64 * slot..45 + 64 * slot + ink.name.len()]
                .copy_from_slice(ink.name.as_bytes());
        }
        let curve = inks.get(slot).map_or(straight(0, 1000), |ink| ink.curve);
        for (i, v) in curve.iter().enumerate() {
            put(&mut out, 300 + 28 * slot + 2 * i, *v as u16);
        }
    }
    put(&mut out, 412, 20);
    for j in 0..11 {
        let (space, c) = overprints.get(j).copied().unwrap_or((0xffff, [0; 4]));
        put(&mut out, 414 + 10 * j, space);
        for (i, v) in c.iter().enumerate() {
            put(&mut out, 416 + 10 * j + 2 * i, *v);
        }
    }
    out
}

// ---- the default overprint, from the CIE formulas ---------------------------

const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

fn lab_xyz(lab: [f64; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let f = [fy + lab[1] / 500.0, fy, fy - lab[2] / 200.0];
    std::array::from_fn(|i| {
        let t = f[i];
        let v = if t * t * t > 216.0 / 24389.0 {
            t * t * t
        } else {
            (116.0 * t - 16.0) * 27.0 / 24389.0
        };
        v * D50[i]
    })
}

fn xyz_lab(xyz: [f64; 3]) -> [f64; 3] {
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let [fx, fy, fz] = std::array::from_fn(|i| f(xyz[i] / D50[i]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// W ⊙ Π(Pᵢ ⊘ W) as Lab.
fn default_overprint(labs: &[[f64; 3]]) -> [f64; 3] {
    let paper = lab_xyz([100.0, 0.0, 0.0]);
    let mut out = paper;
    for lab in labs {
        let ink = lab_xyz(*lab);
        for c in 0..3 {
            out[c] *= ink[c] / paper[c];
        }
    }
    xyz_lab(out)
}

// ---- the PSD, byte by byte --------------------------------------------------

fn resource(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend(id.to_be_bytes());
    out.extend([0, 0]);
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn length(out: &mut Vec<u8>, value: usize, large: bool) {
    if large {
        out.extend((value as u64).to_be_bytes());
    } else {
        out.extend((value as u32).to_be_bytes());
    }
}

/// A colour-mode-8 PSD (or PSB when `large`) of one opaque gray layer and a
/// Raw composite, with `mode_data` and `resources` (id, body).
fn duotone_psd(large: bool, mode_data: &[u8], resources: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let pixels = (W * H) as usize;
    let mut out = b"8BPS".to_vec();
    out.extend((if large { 2u16 } else { 1 }).to_be_bytes());
    out.extend([0; 6]);
    out.extend(1u16.to_be_bytes());
    out.extend(H.to_be_bytes());
    out.extend(W.to_be_bytes());
    out.extend(8u16.to_be_bytes());
    out.extend(8u16.to_be_bytes());
    out.extend((mode_data.len() as u32).to_be_bytes());
    out.extend(mode_data);
    let body: Vec<u8> = resources
        .iter()
        .flat_map(|(id, b)| resource(*id, b))
        .collect();
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    // One layer: alpha (-1) opaque and gray (0), Raw channels.
    let gray: Vec<u8> = GRAY[..pixels].to_vec();
    let channels: [(i16, Vec<u8>); 2] = [
        (-1, [vec![0, 0], vec![255; pixels]].concat()),
        (0, [vec![0, 0], gray.clone()].concat()),
    ];
    let mut info = 1i16.to_be_bytes().to_vec();
    for v in [0i32, 0, H as i32, W as i32] {
        info.extend(v.to_be_bytes());
    }
    info.extend(2u16.to_be_bytes());
    for (id, data) in &channels {
        info.extend(id.to_be_bytes());
        length(&mut info, data.len(), large);
    }
    info.extend(b"8BIMnorm");
    info.extend([255, 0, 0, 0]);
    let mut extra = 0u32.to_be_bytes().to_vec();
    extra.extend(0u32.to_be_bytes());
    let name = b"Layer 1";
    extra.push(name.len() as u8);
    extra.extend(name);
    while !extra.len().is_multiple_of(4) {
        extra.push(0);
    }
    info.extend((extra.len() as u32).to_be_bytes());
    info.extend(extra);
    for (_, data) in &channels {
        info.extend(data);
    }
    if info.len() % 2 == 1 {
        info.push(0);
    }
    let mut section = Vec::new();
    length(&mut section, info.len(), large);
    section.extend(info);
    section.extend(0u32.to_be_bytes());
    length(&mut out, section.len(), large);
    out.extend(section);
    out.extend(0u16.to_be_bytes());
    out.extend(gray);
    out
}

/// A PSD's header colour mode, colour-mode data and image resources, read
/// by Adobe's framing.
struct Sections {
    mode: u16,
    mode_data: Vec<u8>,
    resources: Vec<(u16, Vec<u8>)>,
}

fn sections(bytes: &[u8]) -> Sections {
    assert_eq!(&bytes[..4], b"8BPS");
    let u32_at = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mode = u16::from_be_bytes([bytes[24], bytes[25]]);
    let mode_length = u32_at(26);
    let mode_data = bytes[30..30 + mode_length].to_vec();
    let at = 30 + mode_length;
    let end = at + 4 + u32_at(at);
    let mut cursor = at + 4;
    let mut resources = Vec::new();
    while cursor < end {
        assert_eq!(&bytes[cursor..cursor + 4], b"8BIM");
        let id = u16::from_be_bytes([bytes[cursor + 4], bytes[cursor + 5]]);
        let name = bytes[cursor + 6] as usize;
        cursor += 6 + ((name + 2) & !1);
        let size = u32_at(cursor);
        resources.push((id, bytes[cursor + 4..cursor + 4 + size].to_vec()));
        cursor += 4 + size + (size & 1);
    }
    Sections {
        mode,
        mode_data,
        resources,
    }
}

fn resource_of(sections: &Sections, id: u16) -> Option<&Vec<u8>> {
    sections
        .resources
        .iter()
        .find(|(r, _)| *r == id)
        .map(|(_, b)| b)
}

// ---- the C API ----------------------------------------------------------------

fn owned_json(out: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

fn report_of(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    owned_json(text, err).unwrap()
}

fn open(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let report = report_of(path);
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let working = CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray_psd(
            cpath(path).as_ptr(),
            0,
            fingerprint.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn options(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_duotone_options(doc, &mut err) };
    owned_json(out, err).unwrap()
}

/// Saves `doc` (its own render as the projection) as `container` (0 PSD, 1
/// PSB, 2 TIFF), layered.
fn save(doc: *const PrecisionDocument, path: &Path, container: i32) -> Result<(), String> {
    let projection = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            container,
            0,
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

fn read_ado(path: &Path) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_duotone_read_ado(cpath(path).as_ptr(), &mut err) };
    owned_json(out, err)
}

fn write_ado(path: &Path, options: &Value) -> Result<(), String> {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    if unsafe { dr_duotone_write_ado(cpath(path).as_ptr(), text.as_ptr(), &mut err) } {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The Rust curve with a middle slot: 0 → 10, 50 → 30, 100 → 70 %.
fn rust_curve() -> [i16; 13] {
    let mut curve = straight(100, 700);
    curve[6] = 300;
    curve
}

fn curve_json(curve: &[i16; 13]) -> Value {
    Value::Array(
        curve
            .iter()
            .map(|&v| {
                if v < 0 {
                    Value::Null
                } else {
                    json!(f64::from(v) / 10.0)
                }
            })
            .collect(),
    )
}

/// Each JSON curve slot within `bound` of the expected one (null for null).
fn curves_near(actual: &Value, expected: &Value, bound: f64) {
    let (a, e) = (actual.as_array().unwrap(), expected.as_array().unwrap());
    assert_eq!(a.len(), 13);
    for (x, y) in a.iter().zip(e) {
        match (x.as_f64(), y.as_f64()) {
            (None, None) => {}
            (Some(x), Some(y)) => assert!((x - y).abs() <= bound + 1e-9, "{a:?} vs {e:?}"),
            _ => panic!("blank slots differ: {a:?} vs {e:?}"),
        }
    }
}

fn labs_near(actual: &Value, expected: [f64; 3], bound: f64) {
    for (i, e) in expected.iter().enumerate() {
        let a = actual[i].as_f64().unwrap();
        assert!((a - e).abs() <= bound + 1e-9, "{actual} vs {expected:?}");
    }
}

// ---- the tests ------------------------------------------------------------------

#[test]
fn a_crafted_duotone_psd_and_psb_open_as_duotone_documents() {
    let dir = TempDir::new().unwrap();
    let block = spec(&[warm_black(), rust(rust_curve())], &[]);
    for large in [false, true] {
        let path = write(
            &dir,
            if large { "d.psb" } else { "d.psd" },
            &duotone_psd(large, &block, &[(1018, vec![1, 2, 3, 4, 5])]),
        );
        let report = report_of(&path);
        assert_eq!(report["import_precision"], "gray", "{report}");
        assert_eq!(report["color_mode"], "Duotone");
        assert_eq!(report["duotone"]["type"], "duotone");
        assert_eq!(report["layered_available"], true, "{report}");
        let doc = open(&path).unwrap();
        let got = options(doc);
        assert_eq!(got["type"], "duotone");
        assert_eq!(got["inks"][0]["name"], "Warm Black");
        assert_eq!(got["inks"][0]["lab"], json!([20.0, 2.0, 4.0]));
        assert_eq!(got["inks"][0]["curve"], curve_json(&straight(0, 1000)));
        assert_eq!(got["inks"][1]["name"], "Rust");
        assert_eq!(got["inks"][1]["lab"], json!([55.0, 45.0, 55.0]));
        assert_eq!(got["inks"][1]["curve"], curve_json(&rust_curve()));
        assert_eq!(got["overprints"], json!({"1+2": null}));
        // The pixels are the file's gray.
        let render = unsafe { &*doc }
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap();
        for (pixel, code) in render.iter().zip(GRAY) {
            assert_eq!(pixel[0], u16::from(code) * 257);
        }
        free(doc);
    }
}

#[test]
fn an_unchanged_duotone_writes_its_data_back_and_an_edited_one_re_encodes() {
    let dir = TempDir::new().unwrap();
    let block = spec(&[warm_black(), rust(rust_curve())], &[]);
    let extra = vec![9u8, 8, 7, 6, 5, 4, 3];
    for (large, container) in [(false, 0), (true, 1)] {
        let source = write(
            &dir,
            "src.psd",
            &duotone_psd(large, &block, &[(1018, extra.clone())]),
        );
        let doc = open(&source).unwrap();
        let out = dir.path().join("out.psd");
        save(doc, &out, container).unwrap();
        let written = sections(&std::fs::read(&out).unwrap());
        assert_eq!(written.mode, 8);
        assert_eq!(
            written.mode_data, block,
            "the colour-mode data byte for byte"
        );
        assert_eq!(resource_of(&written, 1018), Some(&extra));
        // It reopens as the same Duotone document.
        let again = open(&out).unwrap();
        assert_eq!(options(again), options(doc));
        free(again);
        // An ink change re-encodes the data and drops the carried resource.
        let mut edited = options(doc);
        edited["inks"][1]["lab"] = json!([60, 40, 50]);
        let next = set(doc, &json!({"inks": edited["inks"]})).unwrap().unwrap();
        save(next, &out, container).unwrap();
        let written = sections(&std::fs::read(&out).unwrap());
        assert_eq!(written.mode, 8);
        assert_ne!(written.mode_data, block);
        assert_eq!(written.mode_data.len(), 524);
        assert_eq!(resource_of(&written, 1018), None);
        let reopened = open(&out).unwrap();
        assert_eq!(
            options(reopened)["inks"][1]["lab"],
            json!([60.0, 40.0, 50.0])
        );
        free(reopened);
        free(next);
        free(doc);
    }
}

#[test]
fn a_layered_tiff_refuses_a_duotone_document() {
    let dir = TempDir::new().unwrap();
    let source = write(
        &dir,
        "src.psd",
        &duotone_psd(false, &spec(&[warm_black(), rust(rust_curve())], &[]), &[]),
    );
    let doc = open(&source).unwrap();
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_psd_export_report(doc, 2, 0, &mut err) };
    let report = owned_json(text, err).unwrap();
    assert_eq!(report["available"], false);
    assert_eq!(report["color_mode"], "duotone");
    let sentence = "A Duotone document is written as a PSD or PSB; a TIFF with layers has no Duotone form. Export as PSD (PSD or PSB), or convert to Grayscale first.";
    assert_eq!(report["refusal"], sentence);
    let error = save(doc, &dir.path().join("x.tif"), 2).unwrap_err();
    assert!(error.contains(sentence), "{error}");
    let psd = unsafe { dr_precision_doc_psd_export_report(doc, 0, 0, &mut err) };
    let psd = owned_json(psd, err).unwrap();
    assert_eq!(psd["available"], true, "{psd}");
    assert_eq!(psd["color_mode"], "duotone");
    free(doc);
}

#[test]
fn a_hand_built_ado_reads_and_writes_back_byte_for_byte() {
    let dir = TempDir::new().unwrap();
    // Photoshop's form with the default overprint written to 0.01, which
    // reads back blank.
    let default = default_overprint(&[[20.0, 2.0, 4.0], [55.0, 45.0, 55.0]]);
    let mut curve = straight(50, 950);
    curve[3] = 250;
    curve[9] = 800;
    let block = spec(&[warm_black(), rust(curve)], &[lab_record(default)]);
    let path = write(&dir, "two.ado", &block);
    let got = read_ado(&path).unwrap();
    assert_eq!(got["type"], "duotone");
    assert_eq!(got["inks"][0]["name"], "Warm Black");
    assert_eq!(got["inks"][1]["curve"], curve_json(&curve));
    assert_eq!(got["overprints"], json!({"1+2": null}));
    labs_near(&got["overprint_display"]["1+2"], default, 1e-6);
    let out = dir.path().join("out.ado");
    write_ado(
        &out,
        &json!({"inks": got["inks"], "overprints": got["overprints"]}),
    )
    .unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), block);
    // A three-ink file and a one-ink one.
    for inks in [
        vec![warm_black()],
        vec![
            warm_black(),
            rust(straight(0, 1000)),
            Ink {
                name: "Lime",
                record: lab_record([80.0, -25.0, 70.0]),
                curve: straight(0, 500),
            },
        ],
    ] {
        let path = write(&dir, "n.ado", &spec(&inks, &[]));
        let got = read_ado(&path).unwrap();
        assert_eq!(got["inks"].as_array().unwrap().len(), inks.len());
        assert!(got["overprints"]
            .as_object()
            .unwrap()
            .values()
            .all(Value::is_null));
    }
}

#[test]
fn an_ado_refuses_by_its_field() {
    let dir = TempDir::new().unwrap();
    let mut cmyk = rust(rust_curve());
    cmyk.record = (2, [0, 0, 0, 0]);
    let path = write(&dir, "cmyk.ado", &spec(&[warm_black(), cmyk], &[]));
    assert_eq!(
        read_ado(&path).unwrap_err(),
        format!(
            "failed to read {}: ink 2's colour is in colour space 2, which Doctored does not read (Lab, RGB, HSB and Gray only)",
            path.display()
        )
    );
    let short = write(&dir, "short.ado", &[0, 1, 0, 2]);
    assert!(read_ado(&short)
        .unwrap_err()
        .contains("4 bytes, shorter than the 524"));
    assert!(read_ado(&dir.path().join("absent.ado"))
        .unwrap_err()
        .starts_with("failed to read"));
    // RGB and Gray ink records read through sRGB and as L*.
    let mut rgb = rust(rust_curve());
    rgb.record = (0, [65535, 65535, 65535, 0]);
    let mut gray = warm_black();
    gray.record = (8, [2500, 0, 0, 0]);
    let path = write(&dir, "rgb.ado", &spec(&[gray, rgb], &[]));
    let got = read_ado(&path).unwrap();
    labs_near(&got["inks"][0]["lab"], [75.0, 0.0, 0.0], 1e-9);
    labs_near(&got["inks"][1]["lab"], [100.0, 0.0, 0.0], 0.05);
}

#[test]
fn blank_overprints_written_read_back_blank_within_the_file_quantization() {
    let dir = TempDir::new().unwrap();
    let gray = gray_doc(&[0, 9000, 33000, 65535], 16, None);
    let mut curve = line(0.0, 100.0);
    curve[4] = json!(33.33);
    let inks = json!([
        {"name": "Warm Black", "lab": [20.004, 2.0049, -3.333], "curve": curve},
        {"name": "Rust", "lab": [55.5, 45.123, 55.0], "curve": line(10.0, 70.04)},
    ]);
    let doc = set(gray, &json!({ "inks": inks })).unwrap().unwrap();
    let path = dir.path().join("q.psd");
    save(doc, &path, 0).unwrap();
    let back = open(&path).unwrap();
    let got = options(back);
    assert_eq!(got["overprints"], json!({"1+2": null}));
    for i in 0..2 {
        let lab = &inks[i]["lab"];
        labs_near(
            &got["inks"][i]["lab"],
            [0, 1, 2].map(|c| lab[c].as_f64().unwrap()),
            0.005,
        );
        curves_near(&got["inks"][i]["curve"], &inks[i]["curve"], 0.05);
    }
    // The .ado writes the same quantization.
    let ado = dir.path().join("q.ado");
    write_ado(&ado, &json!({ "inks": inks })).unwrap();
    let read = read_ado(&ado).unwrap();
    assert_eq!(read["inks"], got["inks"]);
    assert_eq!(read["overprints"], json!({"1+2": null}));
    free(back);
    free(doc);
    free(gray);
}

#[test]
fn a_non_default_overprint_reads_as_authored() {
    let dir = TempDir::new().unwrap();
    let block = spec(
        &[warm_black(), rust(rust_curve())],
        &[lab_record([30.0, 10.0, 20.0])],
    );
    let path = write(&dir, "o.psd", &duotone_psd(false, &block, &[]));
    let doc = open(&path).unwrap();
    assert_eq!(
        options(doc)["overprints"],
        json!({"1+2": [30.0, 10.0, 20.0]})
    );
    free(doc);
}

/// The committed Photoshop PSDs; they ship in the repository, so an absent
/// folder fails rather than skipping.
fn fixtures() -> PathBuf {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/duotone");
    assert!(folder.is_dir(), "{} is absent", folder.display());
    folder
}

#[test]
fn photoshop_duotone_psds_open_with_their_inks_and_round_trip_byte_for_byte() {
    let folder = fixtures();
    let dir = TempDir::new().unwrap();
    for name in ["duotone2-blank.psd", "duotone2-overprint.psd"] {
        let source = folder.join(name);
        let original = sections(&std::fs::read(&source).unwrap());
        assert_eq!(original.mode, 8);
        let doc = open(&source).unwrap();
        let got = options(doc);
        assert_eq!(got["inks"][0]["name"], "Warm Black", "{name}");
        assert_eq!(got["inks"][0]["lab"], json!([20.0, 2.0, 4.0]));
        assert_eq!(got["inks"][1]["name"], "Rust");
        assert_eq!(got["inks"][1]["lab"], json!([55.0, 45.0, 55.0]));
        for i in 0..2 {
            assert_eq!(got["inks"][i]["curve"], curve_json(&straight(0, 1000)));
        }
        // A blank record, and the default product to 0.01, both read blank.
        assert_eq!(got["overprints"], json!({"1+2": null}), "{name}");
        let out = dir.path().join(name);
        save(doc, &out, 0).unwrap();
        let written = sections(&std::fs::read(&out).unwrap());
        assert_eq!(written.mode, 8);
        assert_eq!(written.mode_data, original.mode_data, "{name}");
        for id in [1014, 1017] {
            assert!(resource_of(&original, id).is_some(), "{name} holds {id}");
            assert_eq!(
                resource_of(&written, id),
                resource_of(&original, id),
                "{name} {id}"
            );
        }
        free(doc);
    }
}

/// One ink of Photoshop's parse: name, Lab, 13 curve slots (percent, blank
/// `None`).
type PhotoshopInk = (&'static str, [f64; 3], [Option<f64>; 13]);

/// Case `three`'s `photoshop.inks` in the #1 Duotone phase 2b take
/// (`duotone-mode-547/take1/decided.json`): Photoshop's parse of its own save
/// of the Tritone PSD Doctored wrote, per ink its name, Lab and 13 curve
/// slots (percent, blank `None`). `inks_match` was true, so it equals the
/// stated inks.
const PHOTOSHOP_THREE: &[PhotoshopInk] = &[
    (
        "Warm Black",
        [20.0, 2.0, 4.0],
        [
            Some(0.0),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(100.0),
        ],
    ),
    (
        "Rust",
        [55.0, 45.0, 55.0],
        [
            Some(10.0),
            None,
            None,
            None,
            None,
            None,
            Some(30.0),
            None,
            None,
            None,
            None,
            None,
            Some(70.0),
        ],
    ),
    (
        "Lime",
        [80.0, -25.0, 70.0],
        [
            Some(0.0),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(50.0),
        ],
    ),
];

/// Case `three`'s `differing_ranges` in the same take: Photoshop's
/// colour-mode data against Doctored's, byte-equal, so empty.
const PHOTOSHOP_DIFFERS: &[std::ops::Range<usize>] = &[];

fn curve_of(slots: &[Option<f64>; 13]) -> Value {
    Value::Array(
        slots
            .iter()
            .map(|v| v.map_or(Value::Null, |v| json!(v)))
            .collect(),
    )
}

#[test]
fn photoshops_save_of_a_rasterize_duotone_reads_back() {
    let source = fixtures().join("duotone3-curves.psd");
    let original = sections(&std::fs::read(&source).unwrap());
    assert_eq!(original.mode, 8);
    let doc = open(&source).unwrap();
    let got = options(doc);
    let inks = got["inks"].as_array().unwrap();
    assert_eq!(inks.len(), PHOTOSHOP_THREE.len());
    for (ink, (name, lab, slots)) in inks.iter().zip(PHOTOSHOP_THREE) {
        assert_eq!(ink["name"], *name);
        labs_near(&ink["lab"], *lab, 0.01);
        curves_near(&ink["curve"], &curve_of(slots), 0.1);
    }
    // Saved unchanged: its colour-mode data and 1014/1017 back byte for byte.
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("again.psd");
    save(doc, &out, 0).unwrap();
    free(doc);
    let written = sections(&std::fs::read(&out).unwrap());
    assert_eq!(written.mode, 8);
    assert_eq!(written.mode_data, original.mode_data);
    for id in [1014, 1017] {
        assert!(
            resource_of(&original, id).is_some(),
            "the fixture holds {id}"
        );
        assert_eq!(
            resource_of(&written, id),
            resource_of(&original, id),
            "{id}"
        );
    }
    // A fresh Doctored write of the stated THREE: the same colour-mode data
    // outside the ranges the take found differing.
    let mut rust = vec![Value::Null; 13];
    rust[0] = json!(10.0);
    rust[6] = json!(30.0);
    rust[12] = json!(70.0);
    let three = json!({"inks": [
        {"name": "Warm Black", "lab": [20, 2, 4], "curve": line(0.0, 100.0)},
        {"name": "Rust", "lab": [55, 45, 55], "curve": rust},
        {"name": "Lime", "lab": [80, -25, 70], "curve": line(0.0, 50.0)},
    ]});
    let codes: Vec<u16> = GRAY.iter().map(|&g| u16::from(g)).collect();
    let gray = gray_doc(&codes, 8, None);
    let fresh = set(gray, &three).unwrap().unwrap();
    let ours = dir.path().join("fresh.psd");
    save(fresh, &ours, 0).unwrap();
    free(fresh);
    free(gray);
    let ours = sections(&std::fs::read(&ours).unwrap());
    assert_eq!(ours.mode_data.len(), original.mode_data.len());
    for (i, (a, b)) in ours.mode_data.iter().zip(&original.mode_data).enumerate() {
        if !PHOTOSHOP_DIFFERS.iter().any(|r| r.contains(&i)) {
            assert_eq!(a, b, "colour-mode byte {i}");
        }
    }
}

#[test]
fn every_phase_1b_duotone_export_tolerates_null() {
    let mut err = ptr::null_mut();
    unsafe {
        assert!(dr_precision_doc_duotone_render_plan(ptr::null(), false, &mut err).is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_duotone_render(
            ptr::null(),
            true,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_duotone_to_rgb_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_duotone_to_rgb(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_duotone_read_ado(ptr::null(), &mut err).is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(!dr_duotone_write_ado(ptr::null(), ptr::null(), &mut err));
        dr_string_free(err);
        // NULL err_out is tolerated too.
        assert!(dr_duotone_read_ado(ptr::null(), ptr::null_mut()).is_null());
        assert!(
            dr_precision_doc_duotone_render_plan(ptr::null(), false, ptr::null_mut()).is_null()
        );
    }
}
