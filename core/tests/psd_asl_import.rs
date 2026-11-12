//! Photoshop style libraries (`.asl`) through `dr_asl_decode`.
//!
//! Every library here is an independent byte construction from the shared
//! hand-encoded descriptor builder (`common/psd_descriptor_fixture.rs`),
//! never the crate's writer, and the ORACLE for a style's effects is the PSD
//! importer: the same `Lefx` items written as a PSD record's `lfx2` (or
//! `lmfx`) body open through `dr_doc_open_psd`, and `dr_doc_layer_style`'s
//! style canonicalized as a preset (`dr_style_preset_canonical`) must equal
//! the library's. The Photoshop-authored `.asl` fixtures are tested in
//! `psd_asl_photoshop.rs`.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
use doctored_core::ffi_style::{dr_asl_decode, dr_doc_layer_style, dr_style_preset_canonical};
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::psd_descriptor_fixture as builder;
use crate::psd_mask_fixture as fixture8;
use builder::{body, boolean, desc, enumeration, item, objc, rgbc, text, untf};

const RGBA: [u8; 8] = [11, 29, 47, 255, 101, 151, 199, 255];

// ------------------------------------------------------------- C plumbing --

fn decode(bytes: &[u8]) -> Result<Value, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_decode(bytes.as_ptr(), bytes.len(), &mut err) };
    if raw.is_null() {
        return Err(take_err_string(err));
    }
    Ok(serde_json::from_str(&take_err_string(raw)).expect("report JSON"))
}

fn canonical(json: &str) -> String {
    let style = CString::new(json).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_style_preset_canonical(style.as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    take_err_string(raw)
}

/// The style the PSD importer reads from one record whose `key` block holds
/// `items` (the oracle, module doc), canonicalized as a preset.
fn psd_style(key: &[u8; 4], items: &[Vec<u8>]) -> String {
    psd_style_with(key, items, &[])
}

/// [`psd_style`] with `tail` (framed blocks, a `Patt` for one) appended to
/// the layer-and-mask section after its empty global layer mask.
fn psd_style_with(key: &[u8; 4], items: &[Vec<u8>], tail: &[u8]) -> String {
    let mut block = 0u32.to_be_bytes().to_vec();
    block.extend(16u32.to_be_bytes());
    block.extend(desc(b"null", items));
    if !block.len().is_multiple_of(2) {
        block.push(0);
    }
    let mut record = fixture8::Record::raster([0, 0, 1, 2], &RGBA, false);
    record.name = "Styled";
    record.blocks = fixture8::block(key, &block);
    let mut bytes = fixture8::fixture(&[record], [2, 1], false);
    // The header's 26 bytes, then two empty sections, then the layer-and-mask
    // section's length.
    let at = 26 + 8;
    let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let end = at + 4 + length;
    bytes.splice(end..end, tail.iter().copied());
    bytes[at..at + 4].copy_from_slice(&((length + tail.len()) as u32).to_be_bytes());
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("styled.psd");
    std::fs::write(&path, bytes).unwrap();
    let document = open_psd(&path);
    let raw = unsafe { dr_doc_layer_style(&*document, 0) };
    assert!(!raw.is_null(), "the PSD record carries a style");
    canonical(&take_err_string(raw))
}

fn open_psd(path: &Path) -> Box<DrDocument> {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    let report: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    unsafe { Box::from_raw(raw) }
}

// ------------------------------------------------------------ the library --

/// One style: its name and the `Styl` descriptor's items.
struct Style {
    name: &'static str,
    items: Vec<Vec<u8>>,
}

/// A library: `u16` 2, `8BSL`, `u16` 3, an empty pattern section, the count,
/// then each style's length, its identity and `Styl` bodies, padded to 4.
fn library(styles: &[Style]) -> Vec<u8> {
    library_with(&[], styles)
}

/// [`library`] whose pattern section holds `patterns` (framed records).
fn library_with(patterns: &[u8], styles: &[Style]) -> Vec<u8> {
    let mut out = 2u16.to_be_bytes().to_vec();
    out.extend(b"8BSL");
    out.extend(3u16.to_be_bytes());
    out.extend((patterns.len() as u32).to_be_bytes());
    out.extend(patterns);
    out.extend((styles.len() as u32).to_be_bytes());
    for (i, style) in styles.iter().enumerate() {
        let mut record = body(
            16,
            b"null",
            &[text(b"Nm  ", style.name), text(b"Idnt", &format!("id-{i}"))],
        );
        record.extend(body(16, b"Styl", &style.items));
        out.extend((record.len() as u32).to_be_bytes());
        out.extend(&record);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    out
}

fn lefx(items: &[Vec<u8>]) -> Vec<u8> {
    objc(b"Lefx", b"Lefx", items)
}

fn document_mode() -> Vec<u8> {
    enumeration(b"documentMode", b"documentMode", b"RGBColor")
}

fn drop_shadow() -> Vec<u8> {
    objc(
        b"DrSh",
        b"DrSh",
        &[
            boolean(b"enab", 1),
            enumeration(b"Md  ", b"BlnM", b"Mltp"),
            rgbc(b"Clr ", 10.0, 20.0, 30.0),
            untf(b"Opct", b"#Prc", 60.0),
            boolean(b"uglg", 0),
            untf(b"lagl", b"#Ang", 45.0),
            untf(b"Dstn", b"#Pxl", 8.0),
            untf(b"Ckmt", b"#Prc", 20.0),
            untf(b"blur", b"#Pxl", 12.0),
            boolean(b"layerConceals", 1),
        ],
    )
}

fn stroke_items(size: f64, red: f64) -> Vec<Vec<u8>> {
    vec![
        boolean(b"enab", 1),
        enumeration(b"Md  ", b"BlnM", b"Nrml"),
        untf(b"Opct", b"#Prc", 100.0),
        enumeration(b"Styl", b"FStl", b"OutF"),
        enumeration(b"PntT", b"FrFl", b"SClr"),
        untf(b"Sz  ", b"#Pxl", size),
        rgbc(b"Clr ", red, 60.0, 200.0),
    ]
}

/// Two stacked strokes in Photoshop's `frameFXMulti` list.
fn stacked_strokes() -> Vec<u8> {
    let mut value = 2u32.to_be_bytes().to_vec();
    for (size, red) in [(6.0, 30.0), (3.0, 250.0)] {
        value.extend(b"Objc");
        value.extend(desc(b"FrFX", &stroke_items(size, red)));
    }
    item(b"frameFXMulti", b"VlLs", &value)
}

fn stroke_emboss_bevel() -> Vec<u8> {
    objc(
        b"ebbl",
        b"ebbl",
        &[
            boolean(b"enab", 1),
            enumeration(b"hglM", b"BlnM", b"Scrn"),
            rgbc(b"hglC", 255.0, 255.0, 255.0),
            untf(b"hglO", b"#Prc", 75.0),
            enumeration(b"sdwM", b"BlnM", b"Mltp"),
            rgbc(b"sdwC", 0.0, 0.0, 0.0),
            untf(b"sdwO", b"#Prc", 75.0),
            enumeration(b"bvlS", b"BESl", b"strokeEmboss"),
            untf(b"srgR", b"#Prc", 100.0),
            untf(b"blur", b"#Pxl", 4.0),
        ],
    )
}

fn effects_items() -> Vec<Vec<u8>> {
    vec![
        untf(b"Scl ", b"#Prc", 100.0),
        boolean(b"masterFXSwitch", 1),
        drop_shadow(),
        stacked_strokes(),
    ]
}

// ------------------------------------------------------------------ tests --

#[test]
fn a_style_reads_its_effects_as_the_psd_importer_reads_them() {
    let bytes = library(&[Style {
        name: "Shadow and two strokes",
        items: vec![document_mode(), lefx(&effects_items())],
    }]);
    let report = decode(&bytes).expect("a library");
    let styles = report["styles"].as_array().unwrap();
    assert_eq!(styles.len(), 1);
    let style = &styles[0];
    assert_eq!(style["name"], "Shadow and two strokes");
    assert!(style["blending"].is_null(), "{style}");
    assert_eq!(style["dropped"], Value::Array(vec![]), "{style}");
    // The `lmfx` body is Photoshop's own form for stacked instances.
    let oracle = psd_style(b"lmfx", &effects_items());
    assert_eq!(style["style"].to_string(), oracle);
    let types: Vec<&str> = style["style"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["drop_shadow", "stroke", "stroke"]);
}

#[test]
fn a_scaled_style_is_scaled_as_the_psd_importer_scales_it() {
    let mut items = effects_items();
    items[0] = untf(b"Scl ", b"#Prc", 50.0);
    let bytes = library(&[Style {
        name: "Half",
        items: vec![lefx(&items)],
    }]);
    let report = decode(&bytes).unwrap();
    assert_eq!(
        report["styles"][0]["style"].to_string(),
        psd_style(b"lmfx", &items)
    );
}

#[test]
fn blending_options_give_the_mode_opacity_fill_and_knockout() {
    let blending = objc(
        b"blendOptions",
        b"blendOptions",
        &[
            enumeration(b"Md  ", b"BlnM", b"Mltp"),
            untf(b"Opct", b"#Prc", 70.0),
            untf(b"fillOpacity", b"#Prc", 40.0),
            enumeration(b"knockout", b"knockout", b"shallow"),
            boolean(b"blendClipped", 1),
            boolean(b"transparencyShapesLayer", 1),
            boolean(b"layerMaskAsGlobalMask", 0),
            boolean(b"vectorMaskAsGlobalMask", 0),
            boolean(b"pixelsCanBeLinked", 1),
        ],
    );
    let bytes = library(&[Style {
        name: "Blended shadow",
        items: vec![
            document_mode(),
            blending,
            lefx(&[boolean(b"masterFXSwitch", 1), drop_shadow()]),
        ],
    }]);
    let report = decode(&bytes).unwrap();
    let style = &report["styles"][0];
    assert_eq!(style["blending"]["mode"], "multiply");
    assert_eq!(style["blending"]["opacity"].as_f64(), Some(0.7));
    assert_eq!(style["style"]["fill_opacity"].as_f64(), Some(0.4));
    assert_eq!(style["style"]["knockout"], "shallow");
    // A default key is no loss; any other unmapped key is named.
    assert_eq!(
        style["dropped"],
        serde_json::json!(["Blending Options: pixelsCanBeLinked"])
    );
}

#[test]
fn a_stroke_emboss_bevel_is_dropped_and_named_and_the_stroke_kept() {
    let mut stroke = vec![boolean(b"masterFXSwitch", 1), stroke_emboss_bevel()];
    stroke.push(objc(b"FrFX", b"FrFX", &stroke_items(2.0, 200.0)));
    let bytes = library(&[Style {
        name: "Emboss",
        items: vec![lefx(&stroke)],
    }]);
    let report = decode(&bytes).unwrap();
    let style = &report["styles"][0];
    let types: Vec<&str> = style["style"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["stroke"]);
    assert_eq!(
        style["dropped"],
        serde_json::json!([
            "Bevel & Emboss: bevel uses Stroke Emboss, which Doctored does not render"
        ])
    );
}

#[test]
fn switched_off_effects_and_unknown_keys_are_named_and_an_empty_style_is_null() {
    let bytes = library(&[
        Style {
            name: "Off",
            items: vec![lefx(&[boolean(b"masterFXSwitch", 0), drop_shadow()])],
        },
        Style {
            name: "Unknown",
            items: vec![lefx(&[
                boolean(b"masterFXSwitch", 1),
                objc(b"sparkle", b"sparkle", &[boolean(b"enab", 1)]),
                drop_shadow(),
            ])],
        },
    ]);
    let report = decode(&bytes).unwrap();
    let off = &report["styles"][0];
    assert!(off["style"].is_null(), "{off}");
    assert_eq!(
        off["dropped"],
        serde_json::json!(["Drop Shadow: layer effects are switched off"])
    );
    let unknown = &report["styles"][1];
    assert_eq!(
        unknown["dropped"],
        serde_json::json!(["Layer effects: sparkle"])
    );
    assert_eq!(unknown["style"]["effects"][0]["type"], "drop_shadow");
}

/// The `Ptrn` reference to `crate::psd_pattern_overlay::record`'s "Hand"
/// (its Pascal id is `nope`).
fn hand_reference() -> Vec<u8> {
    objc(
        b"Ptrn",
        b"Ptrn",
        &[text(b"Nm  ", "Hand"), text(b"Idnt", "nope")],
    )
}

fn pattern_overlay() -> Vec<u8> {
    objc(
        b"patternFill",
        b"patternFill",
        &[
            boolean(b"enab", 1),
            enumeration(b"Md  ", b"BlnM", b"Nrml"),
            untf(b"Opct", b"#Prc", 80.0),
            hand_reference(),
            boolean(b"Algn", 1),
            untf(b"Scl ", b"#Prc", 150.0),
        ],
    )
}

fn textured_bevel() -> Vec<u8> {
    objc(
        b"ebbl",
        b"ebbl",
        &[
            boolean(b"enab", 1),
            enumeration(b"hglM", b"BlnM", b"Scrn"),
            rgbc(b"hglC", 255.0, 255.0, 255.0),
            untf(b"hglO", b"#Prc", 75.0),
            enumeration(b"sdwM", b"BlnM", b"Mltp"),
            rgbc(b"sdwC", 0.0, 0.0, 0.0),
            untf(b"sdwO", b"#Prc", 75.0),
            enumeration(b"bvlS", b"BESl", b"InrB"),
            untf(b"srgR", b"#Prc", 100.0),
            untf(b"blur", b"#Pxl", 4.0),
            boolean(b"useTexture", 1),
            hand_reference(),
            boolean(b"Algn", 1),
            untf(b"Scl ", b"#Prc", 50.0),
            untf(b"textureDepth", b"#Prc", -40.0),
            boolean(b"InvT", 1),
        ],
    )
}

#[test]
fn a_pattern_overlay_and_a_bevel_texture_read_the_librarys_patterns_as_a_psds() {
    let (w, h) = (4u32, 3u32);
    let r: Vec<u8> = (0..12).map(|i| (i * 20) as u8).collect();
    let g: Vec<u8> = vec![7; 12];
    let b: Vec<u8> = (0..12).map(|i| 250 - i as u8).collect();
    let a: Vec<u8> = (0..12).map(|i| if i % 3 == 0 { 0 } else { 255 }).collect();
    let records = [crate::psd_pattern_overlay::record(
        3,
        w,
        h,
        "Hand",
        &[r, g, b, a],
        false,
    )];
    let items = vec![
        untf(b"Scl ", b"#Prc", 100.0),
        boolean(b"masterFXSwitch", 1),
        pattern_overlay(),
        textured_bevel(),
    ];
    let bytes = library_with(
        &records.concat(),
        &[Style {
            name: "Patterned",
            items: vec![document_mode(), lefx(&items)],
        }],
    );
    let report = decode(&bytes).expect("a library");
    let style = &report["styles"][0];
    assert_eq!(style["dropped"], Value::Array(vec![]), "{style}");
    let effects = style["style"]["effects"].as_array().expect("{style}");
    let overlay = effects
        .iter()
        .find(|e| e["type"] == "pattern_overlay")
        .expect("the overlay is kept");
    assert_eq!(overlay["pattern"]["name"], "Hand");
    let bevel = effects
        .iter()
        .find(|e| e["type"] == "bevel_emboss")
        .expect("the bevel is kept");
    assert_eq!(bevel["texture"]["pattern"]["name"], "Hand", "{bevel}");
    // The oracle: the same items and the same records as a PSD's `Patt`.
    let oracle = psd_style_with(b"lfx2", &items, &crate::psd_pattern_overlay::patt(&records));
    assert_eq!(style["style"].to_string(), oracle);
    // Without the section the references decline, as a PSD's do.
    let bare = library(&[Style {
        name: "Unresolved",
        items: vec![lefx(&items)],
    }]);
    let unresolved = &decode(&bare).unwrap()["styles"][0];
    assert_ne!(unresolved["dropped"], Value::Array(vec![]), "{unresolved}");
}

#[test]
fn a_truncated_or_foreign_or_oversized_library_is_refused_by_its_sentence() {
    let bytes = library(&[Style {
        name: "Whole",
        items: vec![lefx(&effects_items())],
    }]);
    assert!(decode(&bytes).is_ok());
    assert_eq!(
        decode(&bytes[..bytes.len() - 9]).unwrap_err(),
        "style library is shorter than its layout"
    );
    assert_eq!(
        decode(b"8BPS\0\x01").unwrap_err(),
        "file is not a Photoshop style library (.asl)"
    );
    assert_eq!(
        decode(&[]).unwrap_err(),
        "file is not a Photoshop style library (.asl)"
    );
    let mut many = library(&[]);
    let at = many.len() - 4;
    many[at..].copy_from_slice(&1001u32.to_be_bytes());
    assert_eq!(
        decode(&many).unwrap_err(),
        "style library declares 1001 styles, more than the 1000 it may hold"
    );
    // The length is refused before a byte is read: a one-byte buffer stands
    // for a file past the cap.
    let mut err: *mut c_char = ptr::null_mut();
    let too_long = (64usize << 20) + 1;
    let raw = unsafe { dr_asl_decode([0u8].as_ptr(), too_long, &mut err) };
    assert!(raw.is_null());
    assert_eq!(
        take_err_string(err),
        format!("style library is {too_long} bytes, over the 64 MiB limit")
    );
}

// -------------------------------------------------------------- Blend If --

/// One `Blnd` entry (`asl`'s module doc): `Chnl` an `obj ` reference whose
/// one `Enmr` names `channel`, then the two ramps as longs, each ramp
/// `[black min, black max, white min, white max]` under PITerminology's keys
/// (`SrcW` = `srcWhiteMin`, `Srcm` = `srcWhiteMax`; `DstW`, `Dstt` likewise),
/// the order Photoshop's own library holds (`psd_asl_photoshop.rs`).
fn blnd_entry(channel: &[u8], this_layer: [i32; 4], underlying: [i32; 4]) -> Vec<u8> {
    let mut reference = 1u32.to_be_bytes().to_vec();
    reference.extend(b"Enmr");
    reference.extend(builder::uni(""));
    reference.extend(builder::id(b"Chnl"));
    reference.extend(builder::id(b"Chnl"));
    reference.extend(builder::id(channel));
    let [sb, sl, sw, sm] = this_layer;
    let [db, dl, dw, dt] = underlying;
    let items = [
        item(b"Chnl", b"obj ", &reference),
        builder::long(b"SrcB", sb),
        builder::long(b"Srcl", sl),
        builder::long(b"SrcW", sw),
        builder::long(b"Srcm", sm),
        builder::long(b"DstB", db),
        builder::long(b"Dstl", dl),
        builder::long(b"DstW", dw),
        builder::long(b"Dstt", dt),
    ];
    let mut out = b"Objc".to_vec();
    out.extend(desc(b"Blnd", &items));
    out
}

fn blnd(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut list = (entries.len() as u32).to_be_bytes().to_vec();
    for entry in entries {
        list.extend(entry);
    }
    item(b"Blnd", b"VlLs", &list)
}

fn blended(entries: &[Vec<u8>]) -> Value {
    let bytes = library(&[Style {
        name: "Blend If",
        items: vec![
            document_mode(),
            objc(b"blendOptions", b"blendOptions", &[blnd(entries)]),
            lefx(&[boolean(b"masterFXSwitch", 1), drop_shadow()]),
        ],
    }]);
    decode(&bytes).unwrap()["styles"][0].clone()
}

#[test]
fn blend_if_reads_every_restricted_channel_and_names_any_other_shape() {
    const FULL: [i32; 4] = [0, 0, 255, 255];
    let style = blended(&[
        blnd_entry(b"Gry ", FULL, FULL),
        blnd_entry(b"Rd  ", [0, 10, 240, 255], [5, 15, 235, 250]),
        blnd_entry(b"Grn ", FULL, FULL),
        blnd_entry(b"Bl  ", FULL, FULL),
    ]);
    assert_eq!(style["dropped"], Value::Array(vec![]), "{style}");
    assert_eq!(
        style["style"]["blend_if"],
        serde_json::json!({"channel":"red","this_layer":[0,10,240,255],
            "underlying":[5,15,235,250]})
    );
    // Two restricted channels map, in canonical channel order whatever the
    // file's order (gray first, red in `extra`).
    let style = blended(&[
        blnd_entry(b"Rd  ", [0, 10, 240, 255], FULL),
        blnd_entry(b"Grn ", FULL, FULL),
        blnd_entry(b"Gry ", [0, 20, 255, 255], [3, 25, 235, 252]),
    ]);
    assert_eq!(style["dropped"], Value::Array(vec![]), "{style}");
    assert_eq!(
        style["style"]["blend_if"],
        serde_json::json!({"channel":"gray","this_layer":[0,20,255,255],
            "underlying":[3,25,235,252],
            "extra":[{"channel":"red","this_layer":[0,10,240,255],
                "underlying":[0,0,255,255]}]})
    );
    // Every entry at the full ramp is no Blend If and no loss.
    let style = blended(&[blnd_entry(b"Gry ", FULL, FULL)]);
    assert_eq!(style["dropped"], Value::Array(vec![]), "{style}");
    assert_eq!(style["style"]["blend_if"], Value::Null);
    // A channel restricted twice, a CMYK channel, or a ramp out of order is
    // named, and the rest of the style is kept.
    for entries in [
        vec![
            blnd_entry(b"Gry ", [0, 20, 255, 255], FULL),
            blnd_entry(b"Gry ", [0, 10, 240, 255], FULL),
        ],
        vec![
            blnd_entry(b"Gry ", [0, 20, 255, 255], FULL),
            blnd_entry(b"Cyn ", [0, 10, 240, 255], FULL),
        ],
        vec![blnd_entry(b"Gry ", [0, 200, 100, 255], FULL)],
    ] {
        let style = blended(&entries);
        assert_eq!(
            style["dropped"],
            serde_json::json!(["Blending Options: Blnd"]),
            "{style}"
        );
        assert_eq!(style["style"]["blend_if"], Value::Null);
        assert!(style["style"]["effects"].is_array());
    }
}
