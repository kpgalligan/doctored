//! A layer comp's recorded APPEARANCE through PSD (`psd_comps`), through the
//! FFI: what each writer family writes of a comp's style and what the open
//! reads back, and the sentences both reports state.
//!
//! Oracle: the source document's own comps (`dr_doc_layer_comps_json`, its
//! typed twin) and the rule each sentence states. No file here is compared
//! with bytes Doctored wrote earlier.
//!
//! - A comp's opacity, mode, fill and drop shadow are written and read back
//!   by the byte RGB writer and by every typed one (RGB 16, 32-bit float,
//!   Grayscale, Lab, CMYK), and applying the reopened comp restores them.
//! - A PSD comp holds no Blend If, knockout or Blend Interior Effects as
//!   Group: a reopened comp keeps its layer's own, so applying it deletes
//!   none, and a comp whose recorded three differ from the layer's is named
//!   by the export report.
//! - A record whose style is rasterized for the file (a mask with feather
//!   beside effects) writes no style in any comp setting, with a sentence, so
//!   applying the reopened comp adds no second copy of the effects.
//! - A comp style the effects codec refuses is written as no style, with its
//!   sentence.
//! - A comp's pattern rides in the file's pattern table even when no layer
//!   uses it now.
//! - The import report's sentences: a comp that records nothing, recorded
//!   positions, a style that does not decode, a setting the chain does not
//!   read, settings that do not decode (a non-finite opacity among them; the
//!   opened document still saves and reopens as `.doctored`), and comps past
//!   the `LCMP` payload cap.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::{
    dr_doc_edit_layer_comps, dr_doc_layer_comps_json, dr_precision_doc_layer_comps_json,
};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_mask::dr_doc_set_mask_properties;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{dr_doc_psd_export_report, dr_precision_doc_psd_export_report};
use doctored_core::ffi_style::dr_doc_set_layer_style;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use crate::psd_mode_exchange as modes;
use crate::psd_paths::{byte_open, byte_save, typed_open, typed_save};
use common::psd_photoshop::{import_report, strings};
use common::{add_layer, cpath, doc_from, solid, take_err_string};

const PSD: i32 = 0;
const LAYERS: i32 = 0;

const REFUSED: &str = "1 layer comp records a layer style a PSD layer comp cannot hold; it is written without that style.";
const RASTERIZED: &str = "1 layer comp records the appearance of a layer this file stores as rasterized pixels (a rasterized layer style, or an adjustment layer written as a pixel layer); it is written without what those pixels already hold, so applying it leaves that layer as the file stores it.";
const KEYS: &str = "1 layer comp records a Blend If, knockout or Blend Interior Effects as Group setting that differs from its layer's current one; a PSD layer comp holds none of the three, so applying it after reopening this file keeps the layer's current ones.";
const UNREADABLE: &str = "1 layer record carries layer comp settings that could not be read; that layer is hidden by every imported comp.";

// ------------------------------------------------------------ plumbing --

/// `doc` after `op`, which frees nothing: the old handle is freed here.
fn step(doc: *mut DrDocument, next: *mut DrDocument, what: &str) -> *mut DrDocument {
    assert!(!next.is_null(), "{what}");
    unsafe { dr_doc_free(doc) };
    next
}

fn styled(doc: *mut DrDocument, layer: usize, style: Option<&Value>) -> *mut DrDocument {
    // NULL clears the style.
    let text = style.map(|style| CString::new(style.to_string()).unwrap());
    let mut err: *mut c_char = ptr::null_mut();
    let next = unsafe {
        dr_doc_set_layer_style(
            doc,
            layer,
            text.as_ref().map_or(ptr::null(), |text| text.as_ptr()),
            &mut err,
        )
    };
    assert!(!next.is_null(), "style: {}", take_err_string(err));
    step(doc, next, "style")
}

/// One comp request; the exact no-op (NULL with no error) keeps `doc`.
fn comp_edit(doc: *mut DrDocument, request: Value) -> *mut DrDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let next = unsafe { dr_doc_edit_layer_comps(doc, text.as_ptr(), &mut err) };
    if next.is_null() {
        assert!(err.is_null(), "{request}: {}", take_err_string(err));
        return doc;
    }
    step(doc, next, "comp")
}

fn save_comp(doc: *mut DrDocument, name: &str) -> *mut DrDocument {
    comp_edit(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": name}),
    )
}

fn apply(doc: *mut DrDocument, comp: usize) -> *mut DrDocument {
    comp_edit(
        doc,
        json!({"op": "layer_comp", "action": "apply", "comp": comp}),
    )
}

fn comps(doc: *const DrDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_doc_layer_comps_json(doc, &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

fn typed_comps(doc: &PrecisionDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_precision_doc_layer_comps_json(doc, &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

/// Comp `name`'s entry bound to layer index `layer`.
fn entry(comps: &Value, name: &str, layer: usize) -> Value {
    let comp = comps["comps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no comp {name}: {comps}"));
    comp["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["layer"] == json!(layer))
        .unwrap_or_else(|| panic!("{name}: no entry for layer {layer}"))
        .clone()
}

fn comp_names(comps: &Value) -> Vec<String> {
    comps["comps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

/// A style JSON string as a value, `Null` for none.
fn parsed(style: Option<&str>) -> Value {
    style.map_or(Value::Null, |s| serde_json::from_str(s).unwrap())
}

fn entry_style(entry: &Value) -> Value {
    parsed(entry["style"].as_str())
}

fn layer_style(doc: *const DrDocument, layer: usize) -> Value {
    parsed(
        unsafe { &*doc }.layers[layer]
            .style
            .as_ref()
            .map(|s| s.to_json())
            .as_deref(),
    )
}

fn typed_style(doc: &PrecisionDocument, layer: usize) -> Value {
    parsed(doc.layer(layer).unwrap().reported_style().as_deref())
}

/// What a test compares of an appearance: opacity and fill in whole percent,
/// the mode's code, and the enabled drop shadow's distance.
fn look(opacity: f64, blend: &Value, style: &Value) -> (i64, u64, i64, Option<f64>) {
    let shadow = style["effects"].as_array().and_then(|effects| {
        effects
            .iter()
            .find(|e| e["type"] == "drop_shadow" && e["enabled"] != false)
            .and_then(|e| e["distance"].as_f64())
    });
    (
        (opacity * 100.0).round() as i64,
        blend.as_u64().unwrap(),
        (style["fill_opacity"].as_f64().unwrap_or(1.0) * 100.0).round() as i64,
        shadow,
    )
}

fn entry_look(entry: &Value) -> (i64, u64, i64, Option<f64>) {
    look(
        entry["opacity"].as_f64().expect("a recorded opacity"),
        &entry["blend"],
        &entry_style(entry),
    )
}

fn report_losses(raw: *mut c_char, err: *mut c_char) -> Vec<String> {
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(raw) };
    strings(&serde_json::from_str(&text).unwrap(), "losses")
}

fn export_losses(doc: *const DrDocument) -> Vec<String> {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_doc_psd_export_report(doc, PSD, LAYERS, &mut err) };
    report_losses(raw, err)
}

fn typed_export_losses(doc: &PrecisionDocument) -> Vec<String> {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_precision_doc_psd_export_report(doc, PSD, LAYERS, &mut err) };
    report_losses(raw, err)
}

fn import_losses(path: &Path) -> Vec<String> {
    strings(&import_report(path), "verified_losses")
}

fn comp_sentences(losses: &[String]) -> Vec<&String> {
    losses
        .iter()
        .filter(|l| l.to_lowercase().contains("layer comp"))
        .collect()
}

fn promoted(doc: *const DrDocument) -> Box<PrecisionDocument> {
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!wide.is_null(), "{}", take_err_string(err));
    unsafe { Box::from_raw(wide) }
}

fn typed_apply(doc: &PrecisionDocument, comp: usize) -> Option<Box<PrecisionDocument>> {
    let text =
        CString::new(json!({"op": "layer_comp", "action": "apply", "comp": comp}).to_string())
            .unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        assert!(err.is_null(), "apply: {}", take_err_string(err));
        return None;
    }
    Some(unsafe { Box::from_raw(out) })
}

/// A 16 x 12 document: `Bottom`, and the 8 x 6 raster `A` above it.
fn two_layers(dir: &TempDir) -> *mut DrDocument {
    let doc = doc_from(dir, "bottom.png", &solid(16, 12, [10, 20, 30, 255]));
    add_layer(dir, "a.png", doc, 0, &solid(8, 6, [200, 0, 0, 255]), "A")
}

fn shadow(distance: f64) -> Value {
    json!({"effects": [{"type": "drop_shadow", "distance": distance}]})
}

/// Replaces every `find` inside `within` with the same-length `with`; how
/// many it replaced.
fn patch_in(bytes: &mut [u8], within: std::ops::Range<usize>, find: &[u8], with: &[u8]) -> usize {
    assert_eq!(find.len(), with.len());
    let mut count = 0;
    let mut at = within.start;
    while at + find.len() <= within.end {
        if &bytes[at..at + find.len()] == find {
            bytes[at..at + find.len()].copy_from_slice(with);
            count += 1;
            at += find.len();
        } else {
            at += 1;
        }
    }
    count
}

/// The data of the LAST record's `cmls` item (the top layer's settings):
/// after `8BIM`, the key and four flag bytes comes its `u32` length.
fn last_cmls(bytes: &[u8]) -> std::ops::Range<usize> {
    let at = bytes
        .windows(8)
        .rposition(|w| w == b"8BIMcmls")
        .expect("a cmls item");
    let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
    at + 16..at + 16 + len
}

/// Sets comp `index`'s `capturedInfo` (resource 1065, file order).
fn set_captured(bytes: &mut [u8], index: usize, captured: i32) {
    let key = b"capturedInfolong";
    let at = bytes
        .windows(key.len())
        .enumerate()
        .filter(|(_, w)| w == key)
        .map(|(at, _)| at)
        .nth(index)
        .expect("that comp's capturedInfo")
        + key.len();
    bytes[at..at + 4].copy_from_slice(&captured.to_be_bytes());
}

// ------------------------------------------------- every writer family --

#[test]
fn a_comps_appearance_round_trips_through_the_byte_writer() {
    let dir = TempDir::new().unwrap();
    let mut doc = two_layers(&dir);
    doc = styled(
        doc,
        1,
        Some(&json!({"fill_opacity": 0.5,
            "effects": [{"type": "drop_shadow", "distance": 5.0}]})),
    );
    doc = step(
        doc,
        unsafe { dr_doc_with_layer_blend_mode(doc, 1, BlendMode::Multiply.to_c()) },
        "mode",
    );
    doc = step(
        doc,
        unsafe { dr_doc_with_layer_opacity(doc, 1, 0.6) },
        "opacity",
    );
    doc = save_comp(doc, "Styled");
    doc = styled(doc, 1, Some(&shadow(9.0)));
    doc = step(
        doc,
        unsafe { dr_doc_with_layer_blend_mode(doc, 1, BlendMode::Normal.to_c()) },
        "mode",
    );
    doc = step(
        doc,
        unsafe { dr_doc_with_layer_opacity(doc, 1, 1.0) },
        "opacity",
    );
    doc = save_comp(doc, "Other");
    let want = comps(doc);
    assert_eq!(
        entry_look(&entry(&want, "Styled", 1)),
        (60, BlendMode::Multiply.to_c() as u64, 50, Some(5.0))
    );
    assert_eq!(comp_sentences(&export_losses(doc)), Vec::<&String>::new());

    let path = dir.path().join("styles.psd");
    byte_save(doc, &path, PSD);
    assert_eq!(comp_sentences(&import_losses(&path)), Vec::<&String>::new());
    let mut back = byte_open(&path);
    let got = comps(back);
    for name in ["Styled", "Other"] {
        assert_eq!(
            entry_look(&entry(&got, name, 1)),
            entry_look(&entry(&want, name, 1)),
            "{name}"
        );
    }
    // Applying the reopened comp restores what it recorded.
    back = apply(back, 0);
    let layer = &unsafe { &*back }.layers[1];
    assert_eq!(
        look(
            f64::from(layer.opacity),
            &json!(layer.blend.to_c()),
            &layer_style(back, 1)
        ),
        (60, BlendMode::Multiply.to_c() as u64, 50, Some(5.0))
    );
    unsafe { dr_doc_free(back) };
    unsafe { dr_doc_free(doc) };
}

/// `Bottom` under `A`, a 16-bit sRGB document (`psd_mode_exchange`'s).
fn typed_source() -> PrecisionDocument {
    modes::document(vec![
        modes::raster("Bottom", [10_000, 30_000, 50_000, 65535], 65535),
        modes::raster("A", [50_000, 10_000, 20_000, 65535], 65535),
    ])
}

fn typed_styled(doc: &PrecisionDocument, style: &Value) -> Box<PrecisionDocument> {
    modes::edit(
        doc,
        json!({"op": "layer_style", "version": 1, "layer": modes::index(doc, "A"),
               "style": style}),
    )
}

fn typed_blend(doc: &PrecisionDocument, mode: BlendMode) -> Box<PrecisionDocument> {
    modes::edit(
        doc,
        json!({"op": "layer_blend", "version": 1, "layer": modes::index(doc, "A"),
               "blend_mode": mode.to_c()}),
    )
}

fn typed_save_comp(doc: &PrecisionDocument, name: &str) -> Box<PrecisionDocument> {
    modes::edit(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": name}),
    )
}

/// The typed families: RGB 16 (`None`) and each mode's own writer.
const FAMILIES: [Option<modes::Mode>; 9] = [
    None,
    Some(modes::Mode::Float),
    Some(modes::Mode::Gray8),
    Some(modes::Mode::Gray16),
    Some(modes::Mode::Gray32),
    Some(modes::Mode::Lab8),
    Some(modes::Mode::Lab16),
    Some(modes::Mode::Cmyk8),
    Some(modes::Mode::Cmyk16),
];

fn in_family(source: &PrecisionDocument, family: Option<modes::Mode>) -> Box<PrecisionDocument> {
    match family {
        Some(mode) => modes::convert(source, mode),
        None => Box::new(source.clone()),
    }
}

#[test]
fn a_comps_appearance_round_trips_through_every_typed_writer() {
    let dir = TempDir::new().unwrap();
    let source = typed_source();
    // Every family is read before any is judged, so one run names each
    // writer whose comps differ.
    let mut wrong = Vec::new();
    for family in FAMILIES {
        let what = family.map_or("Rgb16".to_string(), |mode| format!("{mode:?}"));
        // A float document's effects blend in Normal.
        let effect =
            |distance: f64| json!({"type": "drop_shadow", "distance": distance, "blend": "normal"});
        let doc = in_family(&source, family);
        let doc = typed_styled(
            &doc,
            &json!({"fill_opacity": 0.5, "effects": [effect(5.0)]}),
        );
        let doc = typed_blend(&doc, BlendMode::Multiply);
        let doc = typed_save_comp(&doc, "Styled");
        let doc = typed_styled(&doc, &json!({"effects": [effect(9.0)]}));
        let doc = typed_blend(&doc, BlendMode::Normal);
        let doc = typed_save_comp(&doc, "Other");
        let a = modes::index(&doc, "A");
        let want = typed_comps(&doc);
        assert_eq!(
            entry_look(&entry(&want, "Styled", a)),
            (100, BlendMode::Multiply.to_c() as u64, 50, Some(5.0)),
            "{what}: the source"
        );
        assert_eq!(
            comp_sentences(&modes::export_losses(&doc)),
            Vec::<&String>::new(),
            "{what}"
        );
        let path = dir.path().join(format!("styles-{what}.psd"));
        modes::save(&doc, &path, PSD);
        assert_eq!(
            comp_sentences(&import_losses(&path)),
            Vec::<&String>::new(),
            "{what}"
        );
        let back = modes::open(&path);
        let a = modes::index(&back, "A");
        let got = typed_comps(&back);
        for name in ["Styled", "Other"] {
            let got = entry_look(&entry(&got, name, a));
            let want = entry_look(&entry(&want, name, modes::index(&doc, "A")));
            if got != want {
                wrong.push(format!(
                    "{what}: {name} reopened {got:?}, recorded {want:?}"
                ));
            }
        }
        // Applying the reopened comp gives what applying the source's gives.
        let applied_look = |doc: &PrecisionDocument| {
            let applied = typed_apply(doc, 0).unwrap_or_else(|| panic!("{what}: apply changes A"));
            let a = modes::index(&applied, "A");
            let layer = applied.layer(a).unwrap();
            look(
                f64::from(layer.opacity()),
                &json!(layer.blend_mode().to_c()),
                &typed_style(&applied, a),
            )
        };
        let applied = applied_look(&doc);
        // A float document's comp applies no style (`precision_comps`).
        if !doc.is_float() {
            assert_eq!(
                applied,
                (100, BlendMode::Multiply.to_c() as u64, 50, Some(5.0)),
                "{what}: the source applied"
            );
        }
        let reopened = applied_look(&back);
        if reopened != applied {
            wrong.push(format!(
                "{what}: applied {reopened:?}, the source {applied:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

// ----------------------------------- Blend If, knockout and the interior --

/// The three a PSD comp does not hold, over a drop shadow.
fn keyed() -> Value {
    json!({"knockout": "shallow", "blend_interior_as_group": true,
           "blend_if": {"channel": "gray", "this_layer": [0, 0, 200, 255],
                        "underlying": [0, 0, 255, 255]},
           "effects": [{"type": "drop_shadow", "distance": 5.0}]})
}

fn keys_of(style: &Value) -> (Value, Value, Value) {
    (
        style["blend_if"].clone(),
        style["knockout"].clone(),
        style["blend_interior_as_group"].clone(),
    )
}

#[test]
fn a_reopened_comp_keeps_its_layers_blend_if_and_knockout() {
    let dir = TempDir::new().unwrap();
    let mut doc = two_layers(&dir);
    doc = styled(doc, 1, Some(&keyed()));
    doc = save_comp(doc, "Look");
    let own = layer_style(doc, 1);
    assert!(own["blend_if"].is_object() && own["knockout"] == "shallow");
    // The comp records the state the document is in: nothing to say.
    assert_eq!(comp_sentences(&export_losses(doc)), Vec::<&String>::new());

    let path = dir.path().join("keys.psd");
    byte_save(doc, &path, PSD);
    let mut back = byte_open(&path);
    assert_eq!(keys_of(&layer_style(back, 1)), keys_of(&own), "the record");
    assert_eq!(
        keys_of(&entry_style(&entry(&comps(back), "Look", 1))),
        keys_of(&own),
        "the comp's entry"
    );
    back = apply(back, 0);
    assert_eq!(keys_of(&layer_style(back, 1)), keys_of(&own), "applied");
    unsafe { dr_doc_free(back) };

    // 16 bits, where the Blend If lives on the layer.
    let wide = promoted(doc);
    let path16 = dir.path().join("keys16.psd");
    typed_save(&wide, &path16);
    let back = typed_open(&path16);
    let a = modes::index(&back, "A");
    assert_eq!(keys_of(&typed_style(&back, a)), keys_of(&own), "16: record");
    assert_eq!(
        keys_of(&entry_style(&entry(&typed_comps(&back), "Look", a))),
        keys_of(&own),
        "16: the comp's entry"
    );
    if let Some(applied) = typed_apply(&back, 0) {
        assert_eq!(
            keys_of(&typed_style(&applied, a)),
            keys_of(&own),
            "16: applied"
        );
    }

    // A comp whose recorded three differ from the layer's current ones: the
    // export says so, and the reopened comp holds the layer's current ones.
    doc = styled(doc, 1, Some(&shadow(5.0)));
    let losses = export_losses(doc);
    assert!(losses.contains(&KEYS.to_string()), "{losses:?}");
    let path = dir.path().join("keys-differ.psd");
    byte_save(doc, &path, PSD);
    let back = byte_open(&path);
    assert_eq!(
        keys_of(&entry_style(&entry(&comps(back), "Look", 1))),
        (Value::Null, Value::Null, Value::Null)
    );
    unsafe { dr_doc_free(back) };
    let wide = promoted(doc);
    let losses = typed_export_losses(&wide);
    assert!(losses.contains(&KEYS.to_string()), "16: {losses:?}");
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------ a baked record --

#[test]
fn a_rasterized_records_comps_write_no_style() {
    let dir = TempDir::new().unwrap();
    let mut doc = two_layers(&dir);
    doc = styled(
        doc,
        1,
        Some(&json!({"fill_opacity": 0.5,
            "effects": [{"type": "drop_shadow", "distance": 5.0}]})),
    );
    doc = step(
        doc,
        unsafe { dr_doc_adding_layer_mask(doc, 1, 0, ptr::null(), 0, 0) },
        "mask",
    );
    // A feathered mask beside effects: the style is rasterized for a PSD.
    doc = step(
        doc,
        unsafe { dr_doc_set_mask_properties(doc, 1, true, 1.0, 2.0) },
        "feather",
    );
    doc = save_comp(doc, "Look");
    let losses = export_losses(doc);
    assert!(
        losses.iter().any(|l| l.contains("are rasterized into")),
        "{losses:?}"
    );

    let path = dir.path().join("baked.psd");
    byte_save(doc, &path, PSD);
    let mut back = byte_open(&path);
    assert_eq!(layer_style(back, 1), Value::Null, "the record's own");
    assert_eq!(
        entry_style(&entry(&comps(back), "Look", 1)),
        Value::Null,
        "the pixels hold the shadow; the comp must not add it again"
    );
    back = apply(back, 0);
    assert_eq!(layer_style(back, 1), Value::Null, "applied");
    unsafe { dr_doc_free(back) };
    assert!(losses.contains(&RASTERIZED.to_string()), "{losses:?}");

    let wide = promoted(doc);
    let losses = typed_export_losses(&wide);
    assert!(losses.contains(&RASTERIZED.to_string()), "16: {losses:?}");
    let path16 = dir.path().join("baked16.psd");
    typed_save(&wide, &path16);
    let back = typed_open(&path16);
    let a = modes::index(&back, "A");
    assert_eq!(typed_style(&back, a), Value::Null, "16: the record's own");
    assert_eq!(
        entry_style(&entry(&typed_comps(&back), "Look", a)),
        Value::Null,
        "16: the comp's entry"
    );
    unsafe { dr_doc_free(doc) };
}

/// An adjustment layer with no PSD record (a Gradient Map) is written as its
/// stamp: a Normal pixel layer at 100 % holding the picture, the layer's own
/// 50 % and Multiply included. A comp's setting for it writes the stamp's
/// own state, with the sentence, so applying the reopened comp does not fade
/// or re-blend the stamp.
#[test]
fn a_stamped_adjustments_comps_write_the_stamps_own_state() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "bottom.png", &solid(16, 12, [10, 20, 30, 255]));
    let mut doc = add_layer(&dir, "adj.png", doc, 0, &solid(16, 12, [0, 0, 0, 0]), "Map");
    unsafe { &mut *doc }.layers[1].meta = Some(
        json!({"type": "adjust", "op": "gradient_map", "params": {"gradient": {"stops": [
            {"position": 0.0, "color": "#102030", "opacity": 1.0},
            {"position": 1.0, "color": "#f0c040", "opacity": 1.0}]}}})
        .to_string(),
    );
    doc = step(
        doc,
        unsafe { dr_doc_with_layer_opacity(doc, 1, 0.5) },
        "opacity",
    );
    doc = step(
        doc,
        unsafe { dr_doc_with_layer_blend_mode(doc, 1, BlendMode::Multiply.to_c()) },
        "mode",
    );
    doc = save_comp(doc, "Look");
    let losses = export_losses(doc);
    assert!(
        losses
            .iter()
            .any(|l| l.contains("written as a pixel layer")),
        "{losses:?}"
    );
    let path = dir.path().join("stamp.psd");
    byte_save(doc, &path, PSD);
    let mut back = byte_open(&path);
    let stamp = |doc: *const DrDocument| {
        let layer = &unsafe { &*doc }.layers[1];
        look(
            f64::from(layer.opacity),
            &json!(layer.blend.to_c()),
            &layer_style(doc, 1),
        )
    };
    let normal = (100, BlendMode::Normal.to_c() as u64, 100, None);
    assert_eq!(stamp(back), normal, "the stamp's record");
    assert_eq!(
        entry_look(&entry(&comps(back), "Look", 1)),
        normal,
        "the comp's entry"
    );
    back = apply(back, 0);
    assert_eq!(stamp(back), normal, "applied");
    unsafe { dr_doc_free(back) };
    assert!(losses.contains(&RASTERIZED.to_string()), "{losses:?}");

    // 16 bits: the same stamp through the typed writer.
    let wide = promoted(doc);
    let losses = typed_export_losses(&wide);
    assert!(losses.contains(&RASTERIZED.to_string()), "16: {losses:?}");
    let path16 = dir.path().join("stamp16.psd");
    typed_save(&wide, &path16);
    let back = typed_open(&path16);
    let map = modes::index(&back, "Map");
    assert_eq!(
        entry_look(&entry(&typed_comps(&back), "Look", map)),
        normal,
        "16: the comp's entry"
    );
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------- a style the codec refuses --

#[test]
fn a_comp_style_the_codec_refuses_is_written_as_no_style() {
    let dir = TempDir::new().unwrap();
    let mut doc = two_layers(&dir);
    // A stroke under one pixel has no PSD effects record.
    doc = styled(
        doc,
        1,
        Some(&json!({"effects": [{"type": "stroke", "size": 0.5}]})),
    );
    doc = save_comp(doc, "Thin");
    doc = styled(doc, 1, None);
    let losses = export_losses(doc);
    assert!(losses.contains(&REFUSED.to_string()), "{losses:?}");
    let path = dir.path().join("refused.psd");
    byte_save(doc, &path, PSD);
    let back = byte_open(&path);
    assert_eq!(entry_style(&entry(&comps(back), "Thin", 1)), Value::Null);
    unsafe { dr_doc_free(back) };
    let wide = promoted(doc);
    let losses = typed_export_losses(&wide);
    assert!(losses.contains(&REFUSED.to_string()), "16: {losses:?}");
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------ a comp's pattern --

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// A pattern overlay of an opaque `w` x `h` tile named `name`.
fn tiled(name: &str, w: u32, h: u32) -> Value {
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|k| {
            [
                (k % 251) as u8,
                (k / 7 % 253) as u8,
                (k % 13 * 19) as u8,
                255,
            ]
        })
        .collect();
    json!({"effects": [{"type": "pattern_overlay",
        "pattern": {"name": name, "width": w, "height": h, "rgba": base64(&rgba)}}]})
}

#[test]
fn a_comps_pattern_no_layer_uses_now_is_in_the_files_pattern_table() {
    let dir = TempDir::new().unwrap();
    let mut doc = two_layers(&dir);
    doc = styled(doc, 1, Some(&tiled("Probe", 4, 3)));
    doc = save_comp(doc, "Tiled");
    let want = entry_style(&entry(&comps(doc), "Tiled", 1));
    assert_eq!(want["effects"][0]["type"], "pattern_overlay");
    doc = styled(doc, 1, None);

    let path = dir.path().join("pattern.psd");
    byte_save(doc, &path, PSD);
    assert_eq!(comp_sentences(&import_losses(&path)), Vec::<&String>::new());
    let back = byte_open(&path);
    let got = entry_style(&entry(&comps(back), "Tiled", 1));
    assert_eq!(got["effects"][0]["pattern"], want["effects"][0]["pattern"]);
    unsafe { dr_doc_free(back) };

    let wide = promoted(doc);
    let path16 = dir.path().join("pattern16.psd");
    typed_save(&wide, &path16);
    assert_eq!(
        comp_sentences(&import_losses(&path16)),
        Vec::<&String>::new(),
        "16"
    );
    let back = typed_open(&path16);
    let got = entry_style(&entry(
        &typed_comps(&back),
        "Tiled",
        modes::index(&back, "A"),
    ));
    assert_eq!(
        got["effects"][0]["pattern"], want["effects"][0]["pattern"],
        "16"
    );
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------- the import's sentences --

/// `Bottom` and `A` (a drop shadow), comp "One", then A moved and comp
/// "Two": the file each import sentence below is read or patched from.
fn two_comps(dir: &TempDir) -> Vec<u8> {
    let mut doc = two_layers(dir);
    doc = styled(doc, 1, Some(&shadow(5.0)));
    doc = save_comp(doc, "One");
    doc = step(
        doc,
        unsafe { dr_doc_with_layer_offset(doc, 1, 3, 2) },
        "move",
    );
    doc = save_comp(doc, "Two");
    let bytes = byte_save(doc, &dir.path().join("two.psd"), PSD);
    unsafe { dr_doc_free(doc) };
    bytes
}

fn written(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// A recorded position (A sat 3, 2 away when One was saved).
#[test]
fn the_import_report_names_a_recorded_position() {
    let dir = TempDir::new().unwrap();
    let base = two_comps(&dir);
    let losses = import_losses(&written(&dir, "moved.psd", &base));
    assert_eq!(
        comp_sentences(&losses),
        [&"Layer comp “One” records a position for 1 layer; Doctored does not import the positions a PSD's layer comps record, so applying it leaves it in place.".to_string()]
    );
}

/// A comp whose `capturedInfo` is 0.
#[test]
fn the_import_report_names_a_comp_that_records_nothing() {
    let dir = TempDir::new().unwrap();
    let mut bytes = two_comps(&dir);
    set_captured(&mut bytes, 0, 0);
    let path = written(&dir, "nothing.psd", &bytes);
    let losses = import_losses(&path);
    assert!(
        losses.contains(&"Layer comp “One” records nothing; it is not imported.".to_string()),
        "{losses:?}"
    );
    let back = byte_open(&path);
    assert_eq!(comp_names(&comps(back)), ["Two"]);
    unsafe { dr_doc_free(back) };
}

/// A recorded style that does not decode: an unknown effect blend mode in
/// every setting of A's record.
#[test]
fn the_import_report_names_a_recorded_style_that_does_not_decode() {
    let dir = TempDir::new().unwrap();
    let mut bytes = two_comps(&dir);
    let within = last_cmls(&bytes);
    assert_eq!(
        patch_in(&mut bytes, within, b"BlnM\0\0\0\0Mltp", b"BlnM\0\0\0\0Zzzz"),
        3,
        "One, Two and the document state"
    );
    let path = written(&dir, "style.psd", &bytes);
    let losses = import_losses(&path);
    for name in ["One", "Two"] {
        assert!(
            losses.contains(&format!("Layer comp “{name}” records a layer style that could not be read for 1 layer; applying the comp keeps that layer's current style.")),
            "{losses:?}"
        );
    }
    let mut back = byte_open(&path);
    let own = layer_style(back, 1);
    assert_eq!(own["effects"][0]["distance"], 5.0);
    back = apply(back, 0);
    assert_eq!(layer_style(back, 1), own, "its current style is kept");
    unsafe { dr_doc_free(back) };
}

/// A key the chain does not read (the item's own `origFXRefPoint` is renamed
/// with the three settings').
#[test]
fn the_import_report_names_a_setting_the_chain_does_not_read() {
    let dir = TempDir::new().unwrap();
    let mut bytes = two_comps(&dir);
    let within = last_cmls(&bytes);
    assert_eq!(
        patch_in(&mut bytes, within, b"FXRefPoint", b"FXRefPoinX"),
        4
    );
    let losses = import_losses(&written(&dir, "unread.psd", &bytes));
    for name in ["One", "Two"] {
        assert!(
            losses.contains(&format!("Layer comp “{name}” records settings beyond visibility, position, opacity, fill, mode and style for 1 layer; they are not imported.")),
            "{losses:?}"
        );
    }
}

#[test]
fn a_non_finite_recorded_opacity_is_settings_that_do_not_decode() {
    let dir = TempDir::new().unwrap();
    let mut bytes = two_comps(&dir);
    let within = last_cmls(&bytes);
    let at = within.start
        + bytes[within]
            .windows(12)
            .position(|w| w == b"OpctUntF#Prc")
            .expect("an Opct")
        + 12;
    bytes[at..at + 8].copy_from_slice(&f64::NAN.to_be_bytes());
    let path = written(&dir, "nan.psd", &bytes);
    // The opened document saves, and its `.doctored` opens again.
    let back = byte_open(&path);
    for comp in comps(back)["comps"].as_array().unwrap() {
        for e in comp["entries"].as_array().unwrap() {
            assert!(e["opacity"].is_null() || e["opacity"].is_number(), "{e}");
        }
    }
    let native = dir.path().join("nan.doctored");
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(back, cpath(&native).as_ptr(), ptr::null(), &mut err) },
        "save: {}",
        take_err_string(err)
    );
    let again = unsafe { dr_doc_open(cpath(&native).as_ptr(), ptr::null(), &mut err) };
    assert!(!again.is_null(), "reopen: {}", take_err_string(err));
    assert_eq!(comp_names(&comps(again)), ["One", "Two"]);
    unsafe { dr_doc_free(again) };
    unsafe { dr_doc_free(back) };
    let losses = import_losses(&path);
    assert!(losses.contains(&UNREADABLE.to_string()), "{losses:?}");
}

/// Five comps whose recorded pattern overlay is about 1.4 MB of style JSON
/// each fit the document's 8 MiB comp payload; a PSD holds the pattern once,
/// so two more comps that the file says record the same appearance (their
/// settings name no style of their own: the chain's) take the list past it.
/// The first of them also records visibility and is imported without its
/// appearance; the second records appearance alone and is not imported.
#[test]
fn comps_past_the_payload_cap_are_named() {
    let dir = TempDir::new().unwrap();
    let mut doc = two_layers(&dir);
    doc = styled(doc, 1, Some(&tiled("Big", 512, 512)));
    for name in ["c1", "c2", "c3", "c4", "c5"] {
        doc = save_comp(doc, name);
    }
    for name in ["v1", "v2"] {
        doc = comp_edit(
            doc,
            json!({"op": "layer_comp", "action": "save", "name": name,
                   "visibility": true, "position": false, "appearance": false}),
        );
    }
    let mut bytes = byte_save(doc, &dir.path().join("big.psd"), PSD);
    unsafe { dr_doc_free(doc) };
    set_captured(&mut bytes, 5, 5);
    set_captured(&mut bytes, 6, 4);
    let path = written(&dir, "past.psd", &bytes);
    let losses = import_losses(&path);
    assert_eq!(
        comp_sentences(&losses),
        [
            &"Layer comp “v1” is imported without the appearance it records: with it, the file's layer comps are past the 8 MiB a document's layer comps may hold.".to_string(),
            &"Layer comp “v2” is not imported: the file's layer comps are past the 8 MiB a document's layer comps may hold.".to_string(),
        ]
    );
    let back = byte_open(&path);
    let got = comps(back);
    assert_eq!(comp_names(&got), ["c1", "c2", "c3", "c4", "c5", "v1"]);
    assert_eq!(
        got["comps"][5]["captures"],
        json!({"visibility": true, "position": false, "appearance": false})
    );
    assert!(entry(&got, "c5", 1)["style"].is_string());
    let native = dir.path().join("past.doctored");
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(back, cpath(&native).as_ptr(), ptr::null(), &mut err) },
        "save: {}",
        take_err_string(err)
    );
    unsafe { dr_doc_free(back) };
}
