//! STACKED effect instances in a PSD — Photoshop's `lmfx`
//! block — both ways, black-box through the C API: Doctored writes a stacked
//! style as `lmfx` alone (Photoshop's form, `stacked-effects-557/take1`) and
//! reads it back equal at 8 and 16 bits; an `lfx2` beside an `lmfx` that is no
//! effects body carries (it used to decline); a list past ten instances
//! declines by name; Photoshop's own take1 files import with the counts and
//! order the take declared; and Doctored's `lmfx` for a style imported from
//! Photoshop's file matches Photoshop's own block field by field.
//!
//! The descriptor reader here is this file's own (Adobe's published framing),
//! never the crate's. Photoshop's list holds the instances TOP FIRST (take1,
//! TOPFIRST); Doctored's array is bottom first.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as PLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::psd_descriptor_fixture::{boolean, desc, item, objc, rgbc};
use crate::psd_mask_fixture as fixture8;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// Photoshop's `lmfx` key order (take1).
const LMFX_ORDER: [&str; 12] = [
    "Scl ",
    "masterFXSwitch",
    "dropShadowMulti",
    "innerShadowMulti",
    "OrGl",
    "solidFillMulti",
    "gradientFillMulti",
    "frameFXMulti",
    "IrGl",
    "ebbl",
    "ChFX",
    "numModifyingFX",
];

// --------------------------------------------------- the descriptor reader --

#[derive(Clone, Debug, PartialEq)]
enum D {
    Obj(String, Vec<(String, D)>),
    List(Vec<D>),
    Num(f64),
    Bool(bool),
    Text(String),
    Enum(String, String),
    Other,
}

struct R<'a>(&'a [u8], usize);

impl R<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let out = &self.0[self.1..self.1 + n];
        self.1 += n;
        out
    }
    fn u32(&mut self) -> u32 {
        u32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn key(&mut self) -> String {
        let n = self.u32() as usize;
        String::from_utf8_lossy(self.take(if n == 0 { 4 } else { n })).into_owned()
    }
    fn unicode(&mut self) -> String {
        let n = self.u32() as usize;
        let units: Vec<u16> = self
            .take(2 * n)
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string()
    }
    fn value(&mut self, kind: &[u8]) -> D {
        match kind {
            b"Objc" | b"GlbO" => self.descriptor(),
            b"VlLs" => {
                let n = self.u32();
                D::List(
                    (0..n)
                        .map(|_| {
                            let k = self.take(4).to_vec();
                            self.value(&k)
                        })
                        .collect(),
                )
            }
            b"doub" => D::Num(f64::from_be_bytes(self.take(8).try_into().unwrap())),
            b"UntF" => {
                self.take(4);
                D::Num(f64::from_be_bytes(self.take(8).try_into().unwrap()))
            }
            b"long" => D::Num(f64::from(i32::from_be_bytes(
                self.take(4).try_into().unwrap(),
            ))),
            b"comp" => {
                self.take(8);
                D::Other
            }
            b"bool" => D::Bool(self.take(1)[0] != 0),
            b"TEXT" => D::Text(self.unicode()),
            b"enum" => {
                let t = self.key();
                D::Enum(t, self.key())
            }
            b"type" | b"GlbC" => {
                self.unicode();
                self.key();
                D::Other
            }
            b"tdta" | b"alis" => {
                let n = self.u32() as usize;
                self.take(n);
                D::Other
            }
            other => panic!("descriptor type {}", String::from_utf8_lossy(other)),
        }
    }
    fn descriptor(&mut self) -> D {
        self.unicode();
        let class = self.key();
        let n = self.u32();
        let items = (0..n)
            .map(|_| {
                let key = self.key();
                let kind = self.take(4).to_vec();
                (key, self.value(&kind))
            })
            .collect();
        D::Obj(class, items)
    }
}

/// An `lfx2` / `lmfx` body: `u32 0`, `u32 16`, then the descriptor.
fn read_body(body: &[u8]) -> D {
    let mut r = R(body, 0);
    assert_eq!(r.u32(), 0, "the leading word");
    assert_eq!(r.u32(), 16, "descriptor version 16");
    r.descriptor()
}

fn items(d: &D) -> &[(String, D)] {
    match d {
        D::Obj(_, items) => items,
        other => panic!("not an object: {other:?}"),
    }
}

fn get<'a>(d: &'a D, key: &str) -> Option<&'a D> {
    items(d).iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn list<'a>(d: &'a D, key: &str) -> &'a [D] {
    match get(d, key) {
        Some(D::List(items)) => items,
        other => panic!("{key} is not a list: {other:?}"),
    }
}

fn colour(item: &D) -> String {
    let clr = get(item, "Clr ").expect("Clr");
    let n = |k: &str| match get(clr, k) {
        Some(D::Num(v)) => v.round() as u8,
        other => panic!("{k}: {other:?}"),
    };
    format!("#{:02x}{:02x}{:02x}", n("Rd  "), n("Grn "), n("Bl  "))
}

// ------------------------------------------------------------ PSD plumbing --

/// The first record block with `key`, verbatim, of the file at `bytes`.
fn block_body(bytes: &[u8], key: &[u8; 4]) -> Option<Vec<u8>> {
    let mut marker = b"8BIM".to_vec();
    marker.extend(key);
    let at = bytes
        .windows(8)
        .position(|window| window == marker.as_slice())?;
    let len = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
    Some(bytes[at + 12..at + 12 + len].to_vec())
}

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

fn open_psd(path: &Path) -> DrDocument {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), 0, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    *unsafe { Box::from_raw(raw) }
}

fn open_typed(path: &Path) -> PrecisionDocument {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "typed open: {}", take_err_string(error));
    let doc = unsafe { (*raw).clone() };
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(raw) };
    doc
}

fn save8(doc: &DrDocument, path: &Path) {
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_doc_save_psd(doc, ptr::null(), cpath(path).as_ptr(), 0, 0, &mut error) };
    assert!(ok, "save: {}", take_err_string(error));
}

fn save16(doc: &PrecisionDocument, path: &Path) {
    let projection = doc.render(LIMIT, 0, None).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(ok, "save16: {}", take_err_string(error));
}

/// A style JSON's enabled effects as `(type, colour)` in array order; a
/// gradient overlay reports its first stop's colour, an inked slot its ink.
fn instances(style: &str) -> Vec<(String, String)> {
    let v: Value = serde_json::from_str(style).unwrap();
    v["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["enabled"] != json!(false))
        .map(|e| {
            let colour = if let Some(ink) = e.get("ink") {
                ink.to_string()
            } else if let Some(c) = e.get("color").filter(|_| e["type"] != "gradient_overlay") {
                c.as_str().unwrap().to_string()
            } else {
                e["gradient"]["stops"][0]["color"]
                    .as_str()
                    .unwrap()
                    .to_string()
            };
            (e["type"].as_str().unwrap().to_string(), colour)
        })
        .collect()
}

// -------------------------------------------------- Doctored's own files --

/// Every repeating kind stacked (hard, local light), plus a single satin.
const STACKED: &str = "{\"effects\":[\
    {\"type\":\"drop_shadow\",\"color\":\"#0000ff\",\"distance\":8,\"size\":0,\"angle\":90,\"use_global_light\":false},\
    {\"type\":\"drop_shadow\",\"color\":\"#ff0000\",\"distance\":4,\"size\":0,\"angle\":90,\"use_global_light\":false},\
    {\"type\":\"inner_shadow\",\"color\":\"#00ff00\",\"distance\":3,\"size\":2,\"angle\":45,\"use_global_light\":false},\
    {\"type\":\"inner_shadow\",\"color\":\"#ff00ff\",\"distance\":5,\"size\":1,\"angle\":135,\"use_global_light\":false},\
    {\"type\":\"stroke\",\"position\":\"outside\",\"size\":4,\"color\":\"#ff0000\"},\
    {\"type\":\"stroke\",\"position\":\"outside\",\"size\":2,\"color\":\"#0000ff\"},\
    {\"type\":\"stroke\",\"position\":\"inside\",\"size\":3,\"color\":\"#00ffff\",\"opacity\":0.5},\
    {\"type\":\"color_overlay\",\"color\":\"#ff0000\",\"opacity\":0.4},\
    {\"type\":\"color_overlay\",\"color\":\"#0000ff\",\"opacity\":0.3},\
    {\"type\":\"gradient_overlay\",\"opacity\":0.5},\
    {\"type\":\"gradient_overlay\",\"opacity\":0.25,\"gradient\":{\"stops\":[\
      {\"position\":0,\"color\":\"#ff0000\",\"opacity\":1},{\"position\":1,\"color\":\"#ffff00\",\"opacity\":1}]}},\
    {\"type\":\"satin\",\"color\":\"#112233\"}]}";

fn byte_doc() -> DrDocument {
    let rect = RgbaImage::from_pixel(24, 16, Rgba([48, 160, 80, 255]));
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(64, 48, Rgba([255; 4])))
        .adding_image_layer(0, rect, "Stacked")
        .and_then(|d| d.with_layer_offset(1, 20, 14))
        .unwrap();
    doc.layers[1].style = Some(Arc::new(LayerStyle::from_json(STACKED).unwrap()));
    doc
}

fn assert_photoshops_form(bytes: &[u8]) {
    assert!(
        block_body(bytes, b"lfx2").is_none(),
        "no lfx2 beside the lmfx"
    );
    let lmfx = read_body(&block_body(bytes, b"lmfx").expect("an lmfx block"));
    let keys: Vec<&str> = items(&lmfx).iter().map(|(k, _)| k.as_str()).collect();
    let mut order = LMFX_ORDER.iter();
    assert!(
        keys.iter().all(|k| order.any(|o| o == k)),
        "Photoshop's key order: {keys:?}"
    );
    assert_eq!(get(&lmfx, "numModifyingFX"), Some(&D::Num(12.0)));
    // Top first: the later instance of the array is list[0].
    let firsts = |key: &str| list(&lmfx, key).iter().map(colour).collect::<Vec<_>>();
    assert_eq!(firsts("dropShadowMulti"), ["#ff0000", "#0000ff"]);
    assert_eq!(firsts("innerShadowMulti"), ["#ff00ff", "#00ff00"]);
    assert_eq!(firsts("frameFXMulti"), ["#00ffff", "#0000ff", "#ff0000"]);
    assert_eq!(firsts("solidFillMulti"), ["#0000ff", "#ff0000"]);
    assert_eq!(list(&lmfx, "gradientFillMulti").len(), 2);
    assert!(
        matches!(get(&lmfx, "ChFX"), Some(D::Obj(..))),
        "a single kind stays an object"
    );
}

#[test]
fn a_stacked_style_writes_lmfx_alone_and_reopens_equal_at_8_and_16_bits() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc();
    let path = dir.path().join("stacked8.psd");
    save8(&doc, &path);
    assert_photoshops_form(&std::fs::read(&path).unwrap());
    let back = open_psd(&path);
    let style = |d: &DrDocument| d.layers[1].style.as_ref().map(|s| s.to_json());
    assert_eq!(
        instances(&style(&back).unwrap()),
        instances(&style(&doc).unwrap())
    );
    assert_eq!(style(&back), style(&doc), "every field round-trips");

    let s = 257u16;
    let back16 =
        PrecisionImage::from_rgba16(64, 48, vec![[65535; 4]; 64 * 48], IccProfile::srgb(), None)
            .unwrap();
    let rect = PrecisionImage::from_rgba16(
        24,
        16,
        vec![[48 * s, 160 * s, 80 * s, 65535]; 24 * 16],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let style16 = LayerStyle::from_json(STACKED).unwrap();
    let typed = PrecisionDocument::from_parts(
        64,
        48,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            PLayer::raster("Back", back16, (0, 0), 1.0, true).unwrap(),
            PLayer::raster("Stacked", rect, (20, 14), 1.0, true)
                .unwrap()
                .with_layer_style(Some(&style16), (64, 48))
                .unwrap(),
        ],
    )
    .unwrap();
    let path16 = dir.path().join("stacked16.psd");
    save16(&typed, &path16);
    assert_photoshops_form(&std::fs::read(&path16).unwrap());
    let reopened = open_typed(&path16);
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        typed.layer(1).unwrap().reported_style(),
        "the 16-bit instances round-trip"
    );
}

#[test]
fn an_unstacked_style_still_writes_its_lfx2() {
    let dir = TempDir::new().unwrap();
    let mut doc = byte_doc();
    doc.layers[1].style = Some(Arc::new(
        LayerStyle::from_json("{\"effects\":[{\"type\":\"stroke\",\"size\":2}]}").unwrap(),
    ));
    let path = dir.path().join("single.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(block_body(&bytes, b"lfx2").is_some());
    assert!(block_body(&bytes, b"lmfx").is_none());
}

// ---------------------------------------------------- authored fixtures --

fn authored(blocks: Vec<u8>) -> Vec<u8> {
    let rgba = [11, 29, 47, 255, 101, 151, 199, 255];
    let mut record = fixture8::Record::raster([0, 0, 1, 2], &rgba, false);
    record.name = "Styled";
    record.blocks = blocks;
    fixture8::fixture(&[record], [2, 1], false)
}

fn framed(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = 0u32.to_be_bytes().to_vec();
    out.extend(16u32.to_be_bytes());
    out.extend(desc(b"null", items));
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

fn overlay_item(red: f64) -> Vec<u8> {
    let mut out = b"Objc".to_vec();
    out.extend(desc(
        b"SoFi",
        &[boolean(b"enab", 1), rgbc(b"Clr ", red, 0.0, 0.0)],
    ));
    out
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn an_lfx2_beside_an_lmfx_that_is_no_effects_body_carries() {
    // The fixture that used to decline as "instances are stacked (lmfx)": the
    // `lmfx` does not frame, so the `lfx2` carries.
    let dir = TempDir::new().unwrap();
    let mut blocks = fixture8::block(
        b"lfx2",
        &framed(&[
            boolean(b"masterFXSwitch", 1),
            objc(b"SoFi", b"SoFi", &[boolean(b"enab", 1)]),
        ]),
    );
    blocks.extend(fixture8::block(b"lmfx", &[0; 4]));
    let path = write(&dir, "carry.psd", &authored(blocks));
    let doc = open_psd(&path);
    let style = doc.layers[0].style.as_ref().expect("carried").to_json();
    assert_eq!(instances(&style).len(), 1);
}

#[test]
fn an_lmfx_list_of_more_than_ten_declines_by_name_and_ten_carry() {
    let dir = TempDir::new().unwrap();
    for (count, carries) in [(10usize, true), (11, false)] {
        let mut list = (count as u32).to_be_bytes().to_vec();
        for i in 0..count {
            list.extend(overlay_item(20.0 * i as f64));
        }
        let body = framed(&[
            boolean(b"masterFXSwitch", 1),
            item(b"solidFillMulti", b"VlLs", &list),
        ]);
        let path = write(
            &dir,
            &format!("list{count}.psd"),
            &authored(fixture8::block(b"lmfx", &body)),
        );
        let report = import_report(&path);
        let losses = report["verified_losses"].to_string();
        let doc = open_psd(&path);
        if carries {
            let style = doc.layers[0].style.as_ref().expect("ten carry").to_json();
            let reds: Vec<String> = instances(&style).into_iter().map(|(_, c)| c).collect();
            assert_eq!(reds.len(), 10);
            assert_eq!(reds[0], "#b40000", "list[0] is the TOP: the array's last");
            assert_eq!(reds[9], "#000000");
        } else {
            assert!(
                losses.contains("list holds more than 10 instances of one effect"),
                "{losses}"
            );
            assert!(doc.layers[0].style.is_none());
        }
    }
}

// ------------------------------------------------- Photoshop's take1 files --

fn take1() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = Path::new(&home).join("devel/doctored-fixtures/photoshop/stacked-effects-557/take1");
    path.join("judged.json").is_file().then_some(path)
}

fn rgb_cases() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    let pair = |kind| vec![(kind, "#0000ff"), (kind, "#ff0000")];
    vec![
        ("co2", pair("color_overlay")),
        ("st2", pair("stroke")),
        ("ds2", pair("drop_shadow")),
        ("is2", pair("inner_shadow")),
        (
            "go2",
            vec![
                ("gradient_overlay", "#ff0000"),
                ("gradient_overlay", "#000000"),
            ],
        ),
        ("in2", pair("stroke")),
        (
            "mix",
            [pair("drop_shadow"), pair("stroke"), pair("color_overlay")].concat(),
        ),
    ]
}

#[test]
fn photoshops_take1_files_import_with_their_counts_and_order() {
    let Some(take) = take1() else {
        eprintln!("skipped: stacked-effects-557/take1 is absent");
        return;
    };
    for (case, want) in rgb_cases() {
        let want: Vec<(String, String)> = want
            .iter()
            .map(|(k, c)| (k.to_string(), c.to_string()))
            .collect();
        let path8 = take.join(format!("jobs/rgb8/{case}_rgb8.psd"));
        let doc = open_psd(&path8);
        let layer = doc
            .layers
            .iter()
            .find(|l| l.name == "FS_source")
            .expect("FS_source");
        let got = instances(&layer.style.as_ref().expect("carried").to_json());
        assert_eq!(got, want, "{case} at 8 bits");
        let path16 = take.join(format!("jobs/rgb16/{case}_rgb16.psd"));
        let typed = open_typed(&path16);
        let index = (0..typed.layer_count())
            .find(|&i| typed.layer(i).unwrap().name() == "FS_source")
            .unwrap();
        let got = instances(
            &typed
                .layer(index)
                .unwrap()
                .reported_style()
                .expect("carried"),
        );
        assert_eq!(got, want, "{case} at 16 bits");
    }
    let blue = json!([1.0, 1.0, 0.0, 0.0]).to_string();
    let red = json!([0.0, 1.0, 1.0, 0.0]).to_string();
    for (case, kind) in [("co2", "color_overlay"), ("st2", "stroke")] {
        let typed = open_typed(&take.join(format!("jobs/cmyk8/{case}_cmyk8.psd")));
        let index = (0..typed.layer_count())
            .find(|&i| typed.layer(i).unwrap().name() == "FS_source")
            .unwrap();
        let got = instances(
            &typed
                .layer(index)
                .unwrap()
                .reported_style()
                .expect("carried"),
        );
        assert_eq!(
            got,
            vec![
                (kind.to_string(), blue.clone()),
                (kind.to_string(), red.clone())
            ],
            "{case} in ink"
        );
    }
}

/// The fields each list item carries a style in, compared value for value
/// (units aside, numbers within 1e-6).
const FIELDS: [&str; 12] = [
    "enab", "Md  ", "Clr ", "Opct", "uglg", "lagl", "Dstn", "Ckmt", "blur", "Styl", "PntT", "Sz  ",
];

fn same(a: &D, b: &D) -> bool {
    match (a, b) {
        (D::Num(x), D::Num(y)) => (x - y).abs() <= 1e-6,
        (D::Obj(c1, i1), D::Obj(c2, i2)) => {
            c1 == c2
                && i1.len() == i2.len()
                && i1
                    .iter()
                    .all(|(k, v)| i2.iter().any(|(k2, v2)| k == k2 && same(v, v2)))
        }
        _ => a == b,
    }
}

#[test]
fn rasterizes_lmfx_matches_photoshops_field_for_field() {
    let Some(take) = take1() else {
        eprintln!("skipped: stacked-effects-557/take1 is absent");
        return;
    };
    let dir = TempDir::new().unwrap();
    for case in ["mix", "is2", "in2"] {
        let source = take.join(format!("jobs/rgb8/{case}_rgb8.psd"));
        let theirs = read_body(&block_body(&std::fs::read(&source).unwrap(), b"lmfx").unwrap());
        let doc = open_psd(&source);
        let path = dir.path().join(format!("{case}.psd"));
        save8(&doc, &path);
        let ours = read_body(&block_body(&std::fs::read(&path).unwrap(), b"lmfx").unwrap());
        let keys = |d: &D| items(d).iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();
        assert_eq!(
            keys(&ours),
            keys(&theirs),
            "{case}: the same keys, in order"
        );
        assert_eq!(get(&ours, "numModifyingFX"), get(&theirs, "numModifyingFX"));
        for key in [
            "dropShadowMulti",
            "innerShadowMulti",
            "solidFillMulti",
            "frameFXMulti",
        ] {
            let (a, b) = (list(&ours, key), list(&theirs, key));
            assert_eq!(a.len(), b.len(), "{case} {key}: the list length");
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                if get(y, "enab") != Some(&D::Bool(true)) {
                    continue;
                }
                for field in FIELDS {
                    if let (Some(p), Some(q)) = (get(x, field), get(y, field)) {
                        assert!(
                            same(p, q),
                            "{case} {key}[{i}] {field}: ours {p:?}, Photoshop {q:?}"
                        );
                    }
                }
            }
        }
    }
}
